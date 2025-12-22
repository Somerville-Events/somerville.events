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

    const NAME_SIMILARITY_THRESHOLD: f64 = 0.95;
    const DESCRIPTION_SIMILARITY_THRESHOLD: f64 = 0.85;
    const LOCATION_SIMILARITY_THRESHOLD: f64 = 0.95;

    for row in potential_duplicates {
        if is_fuzzy_match(&row.name, &event.name, NAME_SIMILARITY_THRESHOLD)
            && is_fuzzy_match(
                &row.full_description,
                &event.full_description,
                DESCRIPTION_SIMILARITY_THRESHOLD,
            )
            && (match (&row.location, &event.location) {
                (Some(loc1), Some(loc2)) => {
                    is_fuzzy_match(loc1, loc2, LOCATION_SIMILARITY_THRESHOLD)
                }
                (None, None) => true,
                _ => false,
            })
        {
            log::info!("Found duplicate {row:?}. Using it instead of {event:?}");
            return Ok(row.id);
        }
    }

    Ok(None)
}

fn is_fuzzy_match(a: &str, b: &str, threshold: f64) -> bool {
    // jaro_winkler returns 0.0 when either string is empty; that is fine for our threshold checks.
    let a_normalized = normalize_for_similarity(a);
    let b_normalized = normalize_for_similarity(b);
    jaro_winkler(&a_normalized, &b_normalized) >= threshold
}

fn normalize_for_similarity(input: &str) -> String {
    let collapsed = input
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();

    collapsed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_punctuation_and_casing() {
        let a = "Dance Therapy! @ 7PM";
        let b = "dance therapy 7pm";
        assert!(is_fuzzy_match(a, b, 0.95));
    }

    #[test]
    fn rejects_dissimilar_text() {
        let a = "Dance Therapy";
        let b = "Community Cleanup";
        assert!(!is_fuzzy_match(a, b, 0.8));
    }

    #[test]
    fn collapses_whitespace() {
        let a = "Dance     Therapy";
        let b = "Dance Therapy";
        assert!(is_fuzzy_match(a, b, 0.99));
    }
}
