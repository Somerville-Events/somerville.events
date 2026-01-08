use crate::features::view::IndexQuery;
use crate::models::{Event, EventSource, EventType, SimpleEvent};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use strsim::jaro_winkler;

#[async_trait]
pub trait EventsRepo: Send + Sync {
    async fn list(
        &self,
        query: IndexQuery,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<SimpleEvent>>;
    async fn get_distinct_locations(&self) -> Result<Vec<String>>;
    async fn get(&self, id: i64) -> Result<Option<Event>>;
    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool>;
    async fn insert(&self, event: &Event) -> Result<i64>;
    async fn delete(&self, id: i64) -> Result<()>;
}

#[async_trait]
impl EventsRepo for sqlx::Pool<sqlx::Postgres> {
    async fn list(
        &self,
        query: IndexQuery,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<SimpleEvent>> {
        let categories: Vec<String> = query
            .event_types
            .iter()
            .map(|c| c.as_ref().to_string())
            .collect();
        let sources: Vec<String> = query
            .source
            .iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let locations: Vec<String> = query.location.clone();
        let free_only = query.free.unwrap_or(false);
        let name_query = query.q.clone();

        let events = sqlx::query_as!(
            SimpleEvent,
            r#"
            WITH filtered_events AS (
                SELECT DISTINCT e.id
                FROM app.events e
                LEFT JOIN app.event_event_types et ON e.id = et.event_id
                WHERE (cardinality($1::text[]) = 0 OR et.event_type_name = ANY($1::text[]))
                AND (cardinality($2::text[]) = 0 OR e.source = ANY($2::text[]))
                AND (cardinality($3::text[]) = 0 OR e.location_name = ANY($3::text[]))
                AND ($4::boolean = false OR e.price = 0 OR e.price IS NULL)
                AND ($5::text IS NULL OR e.name ILIKE ('%' || $5::text || '%'))
                AND ($6::timestamptz IS NULL OR e.start_date >= $6)
                AND ($7::timestamptz IS NULL OR e.start_date <= $7)
            )
            SELECT
                e.id,
                e.name,
                e.start_date,
                e.end_date,
                e.original_location,
                e.location_name,
                COALESCE(array_agg(et.event_type_name ORDER BY et.event_type_name) FILTER (WHERE et.event_type_name IS NOT NULL), '{}') as "event_types!: Vec<EventType>"
            FROM app.events e
            JOIN filtered_events fe ON e.id = fe.id
            LEFT JOIN app.event_event_types et ON e.id = et.event_id
            GROUP BY e.id
            ORDER BY e.start_date ASC NULLS LAST
            "#,
            &categories,
            &sources,
            &locations,
            free_only,
            name_query,
            since,
            until
        )
        .fetch_all(self)
        .await?;

        Ok(events)
    }

    async fn get_distinct_locations(&self) -> Result<Vec<String>> {
        let locations = sqlx::query!(
            r#"
            SELECT DISTINCT location_name
            FROM app.events
            WHERE location_name IS NOT NULL
            ORDER BY location_name
            "#
        )
        .fetch_all(self)
        .await?;

        Ok(locations
            .into_iter()
            .filter_map(|r| r.location_name)
            .collect())
    }

    async fn get(&self, id: i64) -> Result<Option<Event>> {
        let row = sqlx::query!(
            r#"
            SELECT
                e.id,
                e.name,
                e.description,
                e.full_text,
                e.start_date,
                e.end_date,
                e.address,
                e.original_location,
                e.google_place_id,
                e.location_name,
                e.url,
                e.confidence,
                e.age_restrictions,
                e.price,
                e.source,
                COALESCE(array_agg(et.event_type_name ORDER BY et.event_type_name) FILTER (WHERE et.event_type_name IS NOT NULL), '{}') as "event_types!"
            FROM app.events e
            LEFT JOIN app.event_event_types et ON e.id = et.event_id
            WHERE e.id = $1
            GROUP BY e.id
            "#,
            id,
        )
        .fetch_optional(self)
        .await?;

        Ok(row.map(|r| Event {
            id: Some(r.id),
            name: r.name,
            description: r.description,
            full_text: r.full_text,
            start_date: r.start_date,
            end_date: r.end_date,
            address: r.address,
            original_location: r.original_location,
            google_place_id: r.google_place_id,
            location_name: r.location_name,
            event_types: r.event_types.into_iter().map(EventType::from).collect(),
            url: r.url,
            confidence: r.confidence,
            age_restrictions: r.age_restrictions,
            price: r.price,
            source: EventSource::from(r.source),
            external_id: None,
        }))
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
        .fetch_optional(self)
        .await?;

        Ok(insert_result.is_some())
    }

    async fn insert(&self, event: &Event) -> Result<i64> {
        save_event_to_db(self, event).await
    }

    async fn delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM app.events
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(self)
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

    let mut tx = executor.begin().await?;

    let id = sqlx::query_scalar!(
        r#"
            INSERT INTO app.events (
                name,
                description,
                full_text,
                start_date,
                end_date,
                address,
                original_location,
                google_place_id,
                location_name,
                url,
                confidence,
                age_restrictions,
                price,
                source,
                external_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            RETURNING id
            "#,
        event.name,
        event.description,
        event.full_text,
        event.start_date,
        event.end_date,
        event.address,
        event.original_location,
        event.google_place_id,
        event.location_name,
        event.url,
        event.confidence,
        event.age_restrictions,
        event.price,
        event.source.as_ref(),
        event.external_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| anyhow!("Database insert failed: {e}"))?;

    for et in &event.event_types {
        let et_str = et.as_ref();
        sqlx::query!(
            r#"
                INSERT INTO app.event_event_types (event_id, event_type_name)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
            id,
            et_str
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(id)
}

async fn find_duplicate(
    executor: &sqlx::Pool<sqlx::Postgres>,
    event: &Event,
) -> Result<Option<i64>> {
    let rows = sqlx::query!(
            r#"
            SELECT 
                e.id,
                e.name,
                e.description,
                e.full_text,
                e.start_date,
                e.end_date,
                e.address,
                e.original_location,
                e.google_place_id,
                e.location_name,
                e.url,
                e.confidence,
                e.age_restrictions,
                e.price,
                e.source,
                COALESCE(array_agg(et.event_type_name ORDER BY et.event_type_name) FILTER (WHERE et.event_type_name IS NOT NULL), '{}') as "event_types!"
            FROM app.events e
            LEFT JOIN app.event_event_types et ON e.id = et.event_id
            WHERE e.start_date = $1
            AND e.end_date IS NOT DISTINCT FROM $2
            AND e.address IS NOT DISTINCT FROM $3
            GROUP BY e.id
            "#,
            event.start_date,
            event.end_date,
            event.address
        )
        .fetch_all(executor)
        .await?;

    let potential_duplicates: Vec<Event> = rows
        .into_iter()
        .map(|r| Event {
            id: Some(r.id),
            name: r.name,
            description: r.description,
            full_text: r.full_text,
            start_date: r.start_date,
            end_date: r.end_date,
            address: r.address,
            original_location: r.original_location,
            google_place_id: r.google_place_id,
            location_name: r.location_name,
            event_types: r.event_types.into_iter().map(EventType::from).collect(),
            url: r.url,
            confidence: r.confidence,
            age_restrictions: r.age_restrictions,
            price: r.price,
            source: EventSource::from(r.source),
            external_id: None,
        })
        .collect();

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
    let desc_match = jaro_winkler(&a.description, &b.description) > 0.95;
    name_match && desc_match
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn create_event(name: &str, description: &str, address: Option<&str>) -> Event {
        Event {
            id: None,
            name: name.to_string(),
            description: description.to_string(),
            full_text: description.to_string(),
            start_date: Utc.timestamp_opt(1672531200, 0).unwrap(), // 2023-01-01
            end_date: None,
            address: address.map(|s| s.to_string()),
            original_location: address.map(|s| s.to_string()),
            google_place_id: None,
            location_name: None,
            event_types: vec![],
            url: None,
            confidence: 1.0,
            age_restrictions: None,
            price: None,
            source: EventSource::ImageUpload,
            external_id: None,
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

    #[sqlx::test]
    async fn test_event_types_deterministic_order(pool: sqlx::PgPool) -> Result<()> {
        let mut event = create_event("Sorted Types", "Desc", Some("Loc"));
        // Insert in mixed order
        event.event_types = vec![EventType::Social, EventType::Art, EventType::ChildFriendly];
        event.source = EventSource::ImageUpload;

        let id = save_event_to_db(&pool, &event).await?;

        let fetched = pool.get(id).await?.expect("Event not found");

        // Should be sorted alphabetically by the string representation
        // Art, ChildFriendly, Social
        assert_eq!(
            fetched.event_types,
            vec![EventType::Art, EventType::ChildFriendly, EventType::Social]
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_duplicate_aggregation_bug(pool: sqlx::PgPool) -> Result<()> {
        let mut event = create_event("Multi Tag Event", "Desc", Some("Loc"));
        // 2 distinct tags
        event.event_types = vec![EventType::Art, EventType::Music];
        event.source = EventSource::ImageUpload;

        save_event_to_db(&pool, &event).await?;

        // Query with no filter
        let query_all = IndexQuery {
            event_types: vec![],
            source: vec![],
            past: None,
            ..Default::default()
        };
        let events = pool.list(query_all, None, None).await?;
        assert_eq!(events.len(), 1);
        let fetched_event = &events[0];

        // This fails if duplicate aggregation occurs (e.g. 4 tags instead of 2)
        assert_eq!(
            fetched_event.event_types.len(),
            2,
            "Expected 2 tags, got {:?}",
            fetched_event.event_types
        );
        assert!(fetched_event.event_types.contains(&EventType::Art));
        assert!(fetched_event.event_types.contains(&EventType::Music));

        Ok(())
    }

    #[sqlx::test]
    async fn test_list_filtering(pool: sqlx::PgPool) -> Result<()> {
        let art_event = {
            let mut e = create_event("Art Show", "Paintings", Some("Gallery"));
            e.event_types = vec![EventType::Art];
            e.source = EventSource::ImageUpload;
            // 2023-01-01 10:00:00 UTC
            e.start_date = Utc.timestamp_opt(1672567200, 0).unwrap();
            e
        };

        let music_event = {
            let mut e = create_event("Music Gig", "Bands", Some("Club"));
            e.event_types = vec![EventType::Music];
            e.source = EventSource::ImageUpload;
            // 2023-01-01 12:00:00 UTC
            e.start_date = Utc.timestamp_opt(1672574400, 0).unwrap();
            e
        };

        save_event_to_db(&pool, &art_event).await?;
        save_event_to_db(&pool, &music_event).await?;

        // Test Category Filter
        let query = IndexQuery {
            event_types: vec![EventType::Art],
            source: vec![],
            past: None,
            ..Default::default()
        };

        let events = pool.list(query, None, None).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "Art Show");

        // Test Empty Filter (Should return all)
        let query_all = IndexQuery {
            event_types: vec![],
            source: vec![],
            past: None,
            ..Default::default()
        };
        let events_all = pool.list(query_all, None, None).await?;
        assert_eq!(events_all.len(), 2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_full_query_edge_cases(pool: sqlx::PgPool) -> Result<()> {
        // Setup data
        let base_time = Utc.timestamp_opt(1672531200, 0).unwrap(); // 2023-01-01 00:00:00 UTC

        let event_1 = {
            let mut e = create_event("Event 1", "Desc 1", Some("Loc 1"));
            e.event_types = vec![EventType::Art];
            e.source = EventSource::ImageUpload;
            e.start_date = base_time; // 2023-01-01
            e
        };

        let event_2 = {
            let mut e = create_event("Event 2", "Desc 2", Some("Loc 2"));
            e.event_types = vec![EventType::Music];
            e.source = EventSource::ArtsAtTheArmory; // Different source
            e.start_date = base_time + chrono::Duration::days(1); // 2023-01-02
            e
        };

        let event_3 = {
            let mut e = create_event("Event 3", "Desc 3", Some("Loc 3"));
            e.event_types = vec![EventType::Art, EventType::Music]; // Multiple types
            e.source = EventSource::ImageUpload;
            e.start_date = base_time + chrono::Duration::days(2); // 2023-01-03
            e
        };

        let id1 = save_event_to_db(&pool, &event_1).await?;
        let id2 = save_event_to_db(&pool, &event_2).await?;
        let _id3 = save_event_to_db(&pool, &event_3).await?;

        // 1. GET
        // 1.1 Get existing
        let fetched = pool.get(id1).await?;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "Event 1");

        // 1.2 Get non-existent
        let fetched_none = pool.get(999999).await?;
        assert!(fetched_none.is_none());

        // 2. LIST - Source Filtering
        let query_source = IndexQuery {
            event_types: vec![],
            source: vec![EventSource::ArtsAtTheArmory],
            past: None,
            ..Default::default()
        };
        let res_source = pool.list(query_source, None, None).await?;
        assert_eq!(res_source.len(), 1);
        assert_eq!(res_source[0].id, id2);

        // 3. LIST - Category Filtering
        // 3.1 Single category
        let query_art = IndexQuery {
            event_types: vec![EventType::Art],
            source: vec![],
            past: None,
            ..Default::default()
        };
        let res_art = pool.list(query_art, None, None).await?;
        assert_eq!(res_art.len(), 2); // Event 1 and 3

        // 3.2 Multiple categories (OR logic)
        // If I query for Art OR Music, I should get all 3 (since all have at least one)
        let query_multi = IndexQuery {
            event_types: vec![EventType::Art, EventType::Music],
            source: vec![],
            past: None,
            ..Default::default()
        };
        let res_multi = pool.list(query_multi, None, None).await?;
        assert_eq!(res_multi.len(), 3);

        // 4. LIST - Date Range
        // 4.1 Since (After 2023-01-02) - Should get Event 2 (on day) and 3 (after)
        // Note: The query uses >= for since.
        let since_date = base_time + chrono::Duration::days(1);
        let res_since = pool
            .list(IndexQuery::default(), Some(since_date), None)
            .await?;
        assert_eq!(res_since.len(), 2);

        // 4.2 Until (Before 2023-01-02) - Should get Event 1 and 2
        // Note: The query uses <= for until.
        let until_date = base_time + chrono::Duration::days(1);
        let res_until = pool
            .list(IndexQuery::default(), None, Some(until_date))
            .await?;
        assert_eq!(res_until.len(), 2);

        // 4.3 Window (Only 2023-01-02)
        let res_window = pool
            .list(IndexQuery::default(), Some(since_date), Some(until_date))
            .await?;
        assert_eq!(res_window.len(), 1);
        assert_eq!(res_window[0].id, id2);

        // 5. DELETE
        // 5.1 Delete existing
        pool.delete(id1).await?;
        let check_del = pool.get(id1).await?;
        assert!(check_del.is_none());

        // 5.2 Delete non-existent
        let del_err = pool.delete(id1).await; // Already deleted
        assert!(del_err.is_err());

        // 6. DUPLICATE INSERT (Integration)
        // Try inserting event_2 again. Should return id2.
        let dup_id = save_event_to_db(&pool, &event_2).await?;
        assert_eq!(dup_id, id2);

        Ok(())
    }

    #[sqlx::test]
    async fn test_advanced_filtering(pool: sqlx::PgPool) -> Result<()> {
        let base_time = Utc.timestamp_opt(1672531200, 0).unwrap();

        // 1. Free Event
        let event_free = {
            let mut e = create_event("Free Concert", "Music", Some("Park"));
            e.price = Some(0.0);
            e.source = EventSource::CityOfCambridge;
            e.location_name = Some("Central Park".to_string());
            e.start_date = base_time;
            e
        };

        // 2. Paid Event
        let event_paid = {
            let mut e = create_event("Paid Workshop", "Learn", Some("School"));
            e.price = Some(50.0);
            e.source = EventSource::ImageUpload;
            e.location_name = Some("High School".to_string());
            e.start_date = base_time;
            e
        };

        // 3. Specific Source and Location
        let event_specific = {
            let mut e = create_event("Special Gala", "Party", Some("Hotel"));
            e.price = Some(100.0);
            e.source = EventSource::ArtsAtTheArmory;
            e.location_name = Some("The Armory".to_string());
            e.start_date = base_time;
            e
        };

        save_event_to_db(&pool, &event_free).await?;
        save_event_to_db(&pool, &event_paid).await?;
        save_event_to_db(&pool, &event_specific).await?;

        // Test 1: Free Filter
        let query_free = IndexQuery {
            free: Some(true),
            ..Default::default()
        };
        let res_free = pool.list(query_free, None, None).await?;
        assert_eq!(res_free.len(), 1);
        assert_eq!(res_free[0].name, "Free Concert");

        // Test 2: Location Filter
        let query_loc = IndexQuery {
            location: vec!["The Armory".to_string()],
            ..Default::default()
        };
        let res_loc = pool.list(query_loc, None, None).await?;
        assert_eq!(res_loc.len(), 1);
        assert_eq!(res_loc[0].name, "Special Gala");

        // Test 3: Multiple Source Filter
        let query_sources = IndexQuery {
            source: vec![EventSource::CityOfCambridge, EventSource::ArtsAtTheArmory],
            ..Default::default()
        };
        let res_sources = pool.list(query_sources, None, None).await?;
        assert_eq!(res_sources.len(), 2); // Free Concert and Special Gala

        // Test 4: Fuzzy Search
        // "Concert" should match "Free Concert"
        let query_fuzzy = IndexQuery {
            q: Some("Concert".to_string()),
            ..Default::default()
        };
        let res_fuzzy = pool.list(query_fuzzy, None, None).await?;
        assert_eq!(res_fuzzy.len(), 1);
        assert_eq!(res_fuzzy[0].name, "Free Concert");

        // Test 5: Distinct Locations
        let locations = pool.get_distinct_locations().await?;
        assert_eq!(locations.len(), 3);
        assert!(locations.contains(&"Central Park".to_string()));
        assert!(locations.contains(&"High School".to_string()));
        assert!(locations.contains(&"The Armory".to_string()));

        Ok(())
    }
}
