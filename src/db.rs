use crate::models::Event;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use strsim::jaro_winkler;

#[async_trait]
pub trait EventsRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Event>>;
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
    async fn list(&self) -> Result<Vec<Event>> {
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
                event_type,
                confidence
            FROM app.events
            ORDER BY start_date ASC NULLS LAST
            "#
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
                event_type,
                confidence
            FROM app.events
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(event)
    }

    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool> {
        let insert_result = sqlx::query!(
            r#"
            INSERT INTO app.idempotency_keys (idempotency_key)
            VALUES ($1)
            ON CONFLICT DO NOTHING
            RETURNING idempotency_key
            "#,
            idempotency_key
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(insert_result.is_some())
    }

    async fn insert(&self, event: &Event) -> Result<i64> {
        save_event_to_db(&self.pool, event).await
    }

    async fn delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM app.events
            WHERE id = $1
            "#,
            id
        )
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
            confidence
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
        event.name,
        event.full_description,
        event.start_date,
        event.end_date,
        event.location,
        event.event_type,
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
            event_type,
            confidence
        FROM app.events
        WHERE start_date = $1 AND end_date = $2
        "#,
        event.start_date,
        event.end_date
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
    // High threshold for name to avoid false positives (Workshop A vs B)
    // 0.98 matches "Workshop A" vs "Workshop B", so we need > 0.98.
    let name_match = jaro_winkler(&a.name, &b.name) > 0.985;

    let desc_match = jaro_winkler(&a.full_description, &b.full_description) > 0.85;

    let loc_match = match (&a.location, &b.location) {
        (Some(loc1), Some(loc2)) => {
            jaro_winkler(loc1, loc2) > 0.95
                || loc1.to_lowercase().contains(&loc2.to_lowercase())
                || loc2.to_lowercase().contains(&loc1.to_lowercase())
        }
        (None, None) => true,
        _ => false,
    };

    name_match && desc_match && loc_match
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

        // Case 2: Location with Address. "City Hall" vs "City Hall, 93 Highland Ave"
        // Should be duplicate
        let e3 = create_event(
            "City Council Meeting",
            "Regular meeting.",
            Some("City Hall"),
        );
        let e4 = create_event(
            "City Council Meeting",
            "Regular meeting.",
            Some("City Hall, 93 Highland Ave"),
        );
        assert!(is_duplicate(&e3, &e4), "Location with address should match");

        // Case 3: Series events. "Workshop A" vs "Workshop B"
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

        // Case 4: Room numbers
        // Should NOT be duplicate
        let e7 = create_event("Meeting", "Generic meeting.", Some("Room 101"));
        let e8 = create_event("Meeting", "Generic meeting.", Some("Room 102"));
        assert!(
            !is_duplicate(&e7, &e8),
            "Different room numbers should NOT match"
        );
    }
}
