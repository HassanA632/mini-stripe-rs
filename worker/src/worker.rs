use std::time::Duration;

use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::{db, deliver};

const MAX_ATTEMPTS: i32 = 10;

pub async fn run(db_pool: PgPool) {
    info!("worker started");

    let client = Client::new();
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;

        if let Err(e) = poll_once(&db_pool, &client).await {
            warn!("poll_once failed: {e}");
        }
    }
}

async fn poll_once(db_pool: &PgPool, client: &Client) -> Result<(), String> {
    // Enqueue + claim inside one transaction
    let mut tx = db_pool.begin().await.map_err(|e| e.to_string())?;

    db::enqueue_missing_deliveries(&mut tx)
        .await
        .map_err(|e| e.to_string())?;

    let claimed = db::claim_one_due_delivery(&mut tx)
        .await
        .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;

    let Some(job) = claimed else {
        return Ok(()); // nothing to do this tick
    };

    // Build event payload to send (Stripe-ish)
    let event = serde_json::json!({
        "id": job.event_id,
        "type": job.event_type,
        "created_at": job.event_created_at,
        "data": job.event_payload
    });

    let status =
        deliver::post_webhook(&client, &job.endpoint_url, &job.endpoint_secret, &event).await;

    match status {
        Ok(code) if (200..300).contains(&code) => {
            db::mark_delivery_succeeded(db_pool, job.delivery_id)
                .await
                .map_err(|e| e.to_string())?;
            db::maybe_mark_event_delivered(db_pool, job.event_id)
                .await
                .map_err(|e| e.to_string())?;
            info!(
                "delivered event {} to endpoint {} (HTTP {})",
                job.event_id, job.endpoint_id, code
            );
        }
        Ok(code) => {
            db::mark_delivery_failed(
                db_pool,
                job.delivery_id,
                job.attempt_count,
                format!("non-2xx status: {code}"),
            )
            .await
            .map_err(|e| e.to_string())?;
            db::maybe_mark_event_delivered(db_pool, job.event_id)
                .await
                .map_err(|e| e.to_string())?;
            warn!(
                "delivery failed event {} to endpoint {} (HTTP {})",
                job.event_id, job.endpoint_id, code
            );
        }
        Err(err) => {
            db::mark_delivery_failed(db_pool, job.delivery_id, job.attempt_count, err.clone())
                .await
                .map_err(|e| e.to_string())?;
            db::maybe_mark_event_delivered(db_pool, job.event_id)
                .await
                .map_err(|e| e.to_string())?;

            warn!(
                "delivery failed event {} to endpoint {} ({})",
                job.event_id, job.endpoint_id, err
            );
        }
    }

    Ok(())
}
