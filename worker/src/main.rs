mod worker;

use sqlx::PgPool;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set check .env");

    let db = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to Postgres");

    worker::run(db).await;
}
