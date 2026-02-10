use axum::{
    Router,
    routing::{get, post},
};

use crate::{payment_intents, state::AppState};

async fn health() -> &'static str {
    "ok"
}

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/v1/payment_intents",
            post(payment_intents::create_payment_intent),
        )
        .route(
            "/v1/payment_intents/{id}",
            get(payment_intents::get_payment_intent),
        )
        .with_state(state)
}
