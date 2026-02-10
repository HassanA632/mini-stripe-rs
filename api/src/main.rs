use sqlx::PgPool;

use api::state::AppState;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set (check .env)");

    let db = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to Postgres");

    let state = AppState { db };

    let app = api::app::build_app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("API listening on http://localhost:3000");
    axum::serve(listener, app).await.expect("server error");
}
