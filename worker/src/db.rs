use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub struct ClaimedDelivery {
    pub delivery_id: Uuid,
    pub event_id: Uuid,
    pub event_type: String,
    pub event_created_at: DateTime<Utc>,
    pub event_payload: Value,
    pub endpoint_id: Uuid,
    pub endpoint_url: String,
    pub endpoint_secret: String,
    pub attempt_count: i32,
}

// Enqueue deliveries for any (event, endpoint) pairs that don't exist yet.
// This makes sure the worker has something to deliver without adding more logic to the API.
pub async fn enqueue_missing_deliveries(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    // Insert a pending delivery row for each enabled endpoint per event (if missing).
    // *BUT* This assumes events should be delivered to all enabled endpoints.
    sqlx::query!(
        r#"
        INSERT INTO webhook_deliveries (
          id, event_id, webhook_endpoint_id, status, next_attempt_at
        )
        SELECT
          gen_random_uuid(),
          e.id,
          w.id,
          'pending',
          now()
        FROM events_outbox e
        JOIN webhook_endpoints w ON w.is_enabled = true
        WHERE NOT EXISTS (
          SELECT 1
          FROM webhook_deliveries d
          WHERE d.event_id = e.id AND d.webhook_endpoint_id = w.id
        )
        "#
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

// 2) Claim one due delivery (pending and due now) atomically bumping attempt_count.
// We claim it inside the tx to avoid multiple workers doing the same row later.
pub async fn claim_one_due_delivery(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<ClaimedDelivery>, sqlx::Error> {
    // Select one due pending delivery with row level lock.
    let row = sqlx::query!(
        r#"
        SELECT d.id as delivery_id,
               d.event_id,
               d.webhook_endpoint_id,
               d.attempt_count,
               e.event_type,
               e.payload,
               e.created_at as event_created_at,
               w.url as endpoint_url,
               w.secret as endpoint_secret
        FROM webhook_deliveries d
        JOIN events_outbox e ON e.id = d.event_id
        JOIN webhook_endpoints w ON w.id = d.webhook_endpoint_id
        WHERE d.status = 'pending'
          AND (d.next_attempt_at IS NULL OR d.next_attempt_at <= now())
          AND w.is_enabled = true
        ORDER BY d.next_attempt_at NULLS FIRST, d.created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#
    )
    .fetch_optional(&mut **tx)
    .await?;

    let Some(r) = row else {
        return Ok(None);
    };

    // Mark as in_progress + increment attempt count.
    let new_attempt = r.attempt_count + 1;
    sqlx::query!(
        r#"
        UPDATE webhook_deliveries
        SET status = 'in_progress',
            attempt_count = $2,
            last_attempt_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
        r.delivery_id,
        new_attempt
    )
    .execute(&mut **tx)
    .await?;

    Ok(Some(ClaimedDelivery {
        delivery_id: r.delivery_id,
        event_id: r.event_id,
        event_type: r.event_type,
        event_created_at: r.event_created_at,
        event_payload: r.payload,
        endpoint_id: r.webhook_endpoint_id,
        endpoint_url: r.endpoint_url,
        endpoint_secret: r.endpoint_secret,
        attempt_count: new_attempt,
    }))
}

// 3) Mark delivery result after HTTP attempt
pub async fn mark_delivery_succeeded(db: &PgPool, delivery_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE webhook_deliveries
        SET status = 'succeeded',
            next_attempt_at = NULL,
            last_error = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
        delivery_id
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn mark_delivery_failed(
    db: &PgPool,
    delivery_id: Uuid,
    attempt_count: i32,
    error: String,
) -> Result<(), sqlx::Error> {
    const MAX_ATTEMPTS: i32 = 10;

    if attempt_count >= MAX_ATTEMPTS {
        sqlx::query!(
            r#"
            UPDATE webhook_deliveries
            SET status = 'failed',
                next_attempt_at = NULL,
                last_error = $2,
                updated_at = now()
            WHERE id = $1
            "#,
            delivery_id,
            error
        )
        .execute(db)
        .await?;

        return Ok(());
    }

    // Simple exponential backoff with cap
    let delay_secs = (2_i64.pow(attempt_count.min(10) as u32)).min(60);

    sqlx::query!(
        r#"
        UPDATE webhook_deliveries
        SET status = 'pending',
            next_attempt_at = now() + ($2 || ' seconds')::interval,
            last_error = $3,
            updated_at = now()
        WHERE id = $1
        "#,
        delivery_id,
        delay_secs.to_string(),
        error
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn maybe_mark_event_delivered(db: &PgPool, event_id: Uuid) -> Result<(), sqlx::Error> {
    // If there are any deliveries still pending for this event its not done
    let remaining: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::bigint AS "count!"
        FROM webhook_deliveries
        WHERE event_id = $1
          AND status IN ('pending', 'in_progress')
        "#,
        event_id
    )
    .fetch_one(db)
    .await?;

    if remaining == 0 {
        // Mark delivered_at once (idempotent).
        sqlx::query!(
            r#"
            UPDATE events_outbox
            SET delivered_at = COALESCE(delivered_at, now())
            WHERE id = $1
            "#,
            event_id
        )
        .execute(db)
        .await?;
    }

    Ok(())
}
