use crate::models::Event;
use anyhow::{anyhow, Result};
use async_trait::async_trait;

#[async_trait]
pub trait EventsRepo: Send + Sync {
    async fn list(&self) -> Result<Vec<Event>>;
    async fn get(&self, id: i64) -> Result<Option<Event>>;
    async fn claim_idempotency_key(&self, idempotency_key: uuid::Uuid) -> Result<bool>;
    async fn insert(&self, event: &Event) -> Result<i64>;
}

pub struct EventsDatabase {
    pub pool: sqlx::Pool<sqlx::Postgres>,
}

#[async_trait]
impl EventsRepo for EventsDatabase {
    async fn list(&self) -> Result<Vec<Event>> {
        let events = sqlx::query_as::<_, Event>(
            r#"
                    SELECT
                        id,
                        name,
                        full_description,
                        start_date,
                        end_date,
                        location,
                        event_type,
                        additional_details,
                        confidence
                    FROM app.events
                    ORDER BY start_date ASC
                "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(events)
    }

    async fn get(&self, id: i64) -> Result<Option<Event>> {
        let event = sqlx::query_as::<_, Event>(
            r#"
                    SELECT
                        id,
                        name,
                        full_description,
                        start_date,
                        end_date,
                        location,
                        event_type,
                        additional_details,
                        confidence
                    FROM app.events
                    WHERE id = $1
                "#,
        )
        .bind(id)
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
}

pub async fn save_event_to_db<'e, E>(executor: E, event: &Event) -> Result<i64>
where
    E: sqlx::PgExecutor<'e>,
{
    let id = sqlx::query_scalar(
        r#"
        INSERT INTO app.events (
            name,
            full_description,
            start_date,
            end_date,
            location,
            event_type,
            additional_details,
            confidence
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
    )
    .bind(&event.name)
    .bind(&event.full_description)
    .bind(event.start_date)
    .bind(event.end_date)
    .bind(&event.location)
    .bind(&event.event_type)
    .bind(event.additional_details.as_deref())
    .bind(event.confidence)
    .fetch_one(executor)
    .await
    .map_err(|e| anyhow!("Database insert failed: {e}"))?;

    Ok(id)
}
