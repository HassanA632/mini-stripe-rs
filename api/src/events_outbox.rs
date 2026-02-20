use chrono::Utc;
use uuid::Uuid;

pub async fn insert_event(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), sqlx::Error> {
    let event_id = Uuid::new_v4();

    sqlx::query!(
        r#"
        INSERT INTO events_outbox (id, event_type, payload)
        VALUES ($1, $2, $3)
        "#,
        event_id,
        event_type,
        payload
    )
    .execute(executor)
    .await?;

    Ok(())
}
