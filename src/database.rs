use crate::models::{Event, EventType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use strsim::jaro_winkler;

#[async_trait]
pub trait EventsRepo: Send + Sync {
    async fn list(
        &self,
        category: Option<String>,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<Event>>;
    async fn get(&self, id: i64) -> Result<Option<Event>>;
    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool>;
    async fn insert(&self, event: &Event) -> Result<i64>;
    async fn delete(&self, id: i64) -> Result<()>;
}

pub struct EventsDatabase {
    pub pool: sqlx::Pool<sqlx::Postgres>,
}

#[async_trait]
impl EventsRepo for EventsDatabase {
    async fn list(
        &self,
        category: Option<String>,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<Event>> {
        let events = sqlx::query_as!(
            Event,
            r#"
            SELECT
                id,
                name,
                full_description,
                start_date,
                end_date,
                location,
                event_type as "event_type: EventType",
                url,
                confidence
            FROM app.events
            WHERE ($1::text IS NULL OR event_type::text = $1::text)
            AND ($2::timestamptz IS NULL OR start_date >= $2)
            AND ($3::timestamptz IS NULL OR start_date <= $3)
            ORDER BY start_date ASC NULLS LAST
            "#,
            category,
            since,
            until
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(events)
    }

    async fn get(&self, id: i64) -> Result<Option<Event>> {
        let event = sqlx::query_as!(
            Event,
            r#"
            SELECT
                id,
                name,
                full_description,
                start_date,
                end_date,
                location,
                event_type as "event_type: EventType",
                url,
                confidence
            FROM app.events
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(event)
    }

    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool> {
        let insert_result = sqlx::query(
            r#"
            INSERT INTO app.idempotency_keys (idempotency_key)
            VALUES ($1)
            ON CONFLICT DO NOTHING
            RETURNING idempotency_key
            "#,
        )
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await?;

        Ok(insert_result.is_some())
    }

    async fn insert(&self, event: &Event) -> Result<i64> {
        save_event_to_db(&self.pool, event).await
    }

    async fn delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM app.events
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow!("Event with id {} not found", id));
        }

        Ok(())
    }
}

pub async fn save_event_to_db(executor: &sqlx::Pool<sqlx::Postgres>, event: &Event) -> Result<i64> {
    // If the event already exists, instead of saving a new one just
    // return the ID for the existing one.
    if let Some(duplicate_id) = find_duplicate(executor, event)
        .await
        .map_err(|e| anyhow!("Database lookup failed: {e}"))?
    {
        return Ok(duplicate_id);
    }

    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO app.events (
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            url,
            confidence
        )
        VALUES ($1, $2, $3, $4, $5, $6::app.event_type, $7, $8)
        RETURNING id
        "#,
        event.name,
        event.full_description,
        event.start_date,
        event.end_date,
        event.location,
        event.event_type.as_ref() as Option<&EventType>,
        event.url,
        event.confidence
    )
    .fetch_one(executor)
    .await
    .map_err(|e| anyhow!("Database insert failed: {e}"))?;

    Ok(id)
}

async fn find_duplicate(
    executor: &sqlx::Pool<sqlx::Postgres>,
    event: &Event,
) -> Result<Option<i64>> {
    let potential_duplicates = sqlx::query_as!(
        Event,
        r#"
        SELECT 
            id,
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type as "event_type: EventType",
            url,
            confidence
        FROM app.events
        WHERE start_date = $1 
          AND end_date IS NOT DISTINCT FROM $2
          AND location IS NOT DISTINCT FROM $3
        "#,
        event.start_date,
        event.end_date,
        event.location
    )
    .fetch_all(executor)
    .await?;

    for row in potential_duplicates {
        if is_duplicate(&row, event) {
            log::info!("Found duplicate {row:?}. Using it instead of {event:?}");
            return Ok(row.id);
        }
    }

    Ok(None)
}

fn is_duplicate(a: &Event, b: &Event) -> bool {
    // start_date, end_date, and description are equal because of a
    // previous database query.

    // High threshold for name to avoid false positives (Workshop A vs B)
    // 0.98 matches "Workshop A" vs "Workshop B", so we need > 0.98.
    let name_match = jaro_winkler(&a.name, &b.name) > 0.985;
    let desc_match = jaro_winkler(&a.full_description, &b.full_description) > 0.95;
    name_match && desc_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn create_event(name: &str, description: &str, location: Option<&str>) -> Event {
        Event {
            id: None,
            name: name.to_string(),
            full_description: description.to_string(),
            start_date: Utc.timestamp_opt(1672531200, 0).unwrap(), // 2023-01-01
            end_date: None,
            location: location.map(|s| s.to_string()),
            event_type: None,
            url: None,
            confidence: 1.0,
        }
    }

    #[test]
    fn test_duplicate_detection_boundaries() {
        // Case 1: Name typo/extraction noise. "Somerville City Council" vs "Somerville City Councl"
        // Should be duplicate
        let e1 = create_event(
            "Somerville City Council",
            "Regular meeting of the council.",
            Some("City Hall"),
        );
        let e2 = create_event(
            "Somerville City Councl",
            "Regular meeting of the council.",
            Some("City Hall"),
        );
        assert!(is_duplicate(&e1, &e2), "Typo in name should match");

        // Case 2: Series events. "Workshop A" vs "Workshop B"
        // Should NOT be duplicate
        let e5 = create_event(
            "Community Workshop A",
            "Discussion on topic A.",
            Some("Community Center"),
        );
        let e6 = create_event(
            "Community Workshop B",
            "Discussion on topic B.",
            Some("Community Center"),
        );
        assert!(!is_duplicate(&e5, &e6), "Workshop A vs B should NOT match");
    }

    #[test]
    fn test_duplicate_detection_strictness_lateral() {
        // 1. Different Levels of same activity
        let e1 = create_event("Salsa Level 1", "Learn the basics.", Some("Dance Studio"));
        let e2 = create_event("Salsa Level 2", "Intermediate moves.", Some("Dance Studio"));
        assert!(
            !is_duplicate(&e1, &e2),
            "Level 1 vs Level 2 should NOT match"
        );

        // 2. Different Committees at City Hall
        let e3 = create_event(
            "School Committee Meeting",
            "Weekly meeting.",
            Some("City Hall"),
        );
        let e4 = create_event(
            "Finance Committee Meeting",
            "Weekly meeting.",
            Some("City Hall"),
        );
        assert!(
            !is_duplicate(&e3, &e4),
            "Different committees should NOT match"
        );

        // 3. Age Groups
        let e5 = create_event("Youth Soccer (U8)", "Saturday game.", Some("Trum Field"));
        let e6 = create_event("Youth Soccer (U10)", "Saturday game.", Some("Trum Field"));
        assert!(
            !is_duplicate(&e5, &e6),
            "Different age groups should NOT match"
        );

        // 4. Festival Acts (Same location, slightly different description/name)
        let e7 = create_event("Porchfest: Band A", "Live music.", Some("123 Summer St"));
        let e8 = create_event("Porchfest: Band B", "Live music.", Some("123 Summer St"));
        assert!(
            !is_duplicate(&e7, &e8),
            "Different bands at same festival venue should NOT match"
        );

        // 5. Language variations
        let e9 = create_event("Storytime (English)", "Read aloud.", Some("Library"));
        let e10 = create_event("Storytime (Spanish)", "Read aloud.", Some("Library"));
        assert!(
            !is_duplicate(&e9, &e10),
            "Different languages should NOT match"
        );

        // 6. Sports Opponents
        let e11 = create_event(
            "Somerville vs Medford",
            "Varsity Game",
            Some("Dilboy Stadium"),
        );
        let e12 = create_event(
            "Somerville vs Everett",
            "Varsity Game",
            Some("Dilboy Stadium"),
        );
        assert!(
            !is_duplicate(&e11, &e12),
            "Different opponents should NOT match"
        );

        // 7. Ward Meetings
        let e13 = create_event("Ward 1 Meeting", "Community update", Some("Zoom"));
        let e14 = create_event("Ward 2 Meeting", "Community update", Some("Zoom"));
        assert!(
            !is_duplicate(&e13, &e14),
            "Different wards should NOT match"
        );
    }

    #[test]
    fn test_duplicate_detection_complex_scenarios() {
        // Case 2: Generic Titles, Different Descriptions
        let e3 = create_event(
            "Weekly Meeting",
            "Discussing zoning laws for the new park.",
            Some("City Hall"),
        );
        let e4 = create_event(
            "Weekly Meeting",
            "Discussing school budget and teacher salaries.",
            Some("City Hall"),
        );
        // Same name/location, but descriptions are totally different topics
        assert!(
            !is_duplicate(&e3, &e4),
            "Same title but different topics (descriptions) should NOT match"
        );

        // Case 3: Cut-off text leading to ambiguity (Extraction Artifacts)
        let e5 = create_event("Ward Meeting", "Community update.", Some("Library"));
        let e6 = create_event("Ward 2 Meeting", "Community update.", Some("Library"));
        assert!(
            !is_duplicate(&e5, &e6),
            "Generic/Cut-off name should NOT match specific name"
        );

        // Case 4: Truncated names matching unrelated events
        let e7 = create_event("Somerville Art", "Local event.", Some("Armory"));
        let e8 = create_event("Somerville Art Class", "Local event.", Some("Armory"));
        assert!(
            !is_duplicate(&e7, &e8),
            "Prefix match on different event types should NOT match"
        );

        // Case 5: Long Description overlaps but key differences
        let e9 = create_event(
            "Evening Social",
            "Join us for a wonderful night of music, dancing, and light refreshments at the club.",
            Some("The Club"),
        );
        let e10 = create_event(
            "Evening Social",
            "Join us for a wonderful night of painting, wine, and light refreshments at the club.",
            Some("The Club"),
        );
        assert!(
            !is_duplicate(&e9, &e10),
            "Descriptions with key activity differences should NOT match"
        );
    }
}
