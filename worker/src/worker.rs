use std::time::Duration;

use sqlx::PgPool;
use tracing::{info, warn};

pub async fn run(db: PgPool) {
    info!("worker started");

    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;

        if let Err(e) = poll_once(&db).await {
            warn!("poll_once failed: {e}");
        }
    }
}

async fn poll_once(db: &PgPool) -> Result<(), sqlx::Error> {
    let due_count: i64 = sqlx::query_scalar!(
        r#"
    SELECT COUNT(*)::bigint AS "count!"
    FROM webhook_deliveries
    WHERE status = 'pending'
      AND (next_attempt_at IS NULL OR next_attempt_at <= now())
    "#
    )
    .fetch_one(db)
    .await?;

    if due_count > 0 {
        info!("due deliveries: {due_count}");
    }

    Ok(())
}
