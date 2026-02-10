use api::{app::build_app, state::AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn create_then_get_payment_intent(pool: PgPool) {
    // Build the router with real DB pool
    let app = build_app(AppState { db: pool });

    // POST /v1/payment_intents
    let body = json!({ "amount": 1000, "currency": "gbp" }).to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = created["id"].as_str().unwrap();

    // GET /v1/payment_intents/{id}
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/payment_intents/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let fetched: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(fetched["id"], created["id"]);
    assert_eq!(fetched["amount"], 1000);
    assert_eq!(fetched["currency"], "gbp");
    assert_eq!(fetched["status"], "requires_confirmation");
}

#[sqlx::test(migrations = "./migrations")]
async fn get_unknown_payment_intent_returns_404(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let random_id = Uuid::new_v4();

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/payment_intents/{random_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
