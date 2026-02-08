use axum::{Router, routing::get};

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bild to port 3000");

    println!("API listeneing on localhost:3000");
    axum::serve(listener, app).await.expect("Server error");
}
