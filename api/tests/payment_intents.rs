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

#[sqlx::test(migrations = "./migrations")]
async fn idempotency_same_key_same_body_returns_same_intent(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let body = json!({ "amount": 2500, "currency": "gbp" }).to_string();

    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "abc123")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res1.status(), StatusCode::CREATED);
    let bytes1 = res1.into_body().collect().await.unwrap().to_bytes();
    let v1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();
    let id1 = v1["id"].as_str().unwrap().to_string();

    let res2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "abc123")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res2.status(), StatusCode::CREATED);
    let bytes2 = res2.into_body().collect().await.unwrap().to_bytes();
    let v2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
    let id2 = v2["id"].as_str().unwrap().to_string();

    assert_eq!(id1, id2);
}

#[sqlx::test(migrations = "./migrations")]
async fn idempotency_same_key_different_body_returns_409(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let body1 = json!({ "amount": 2500, "currency": "gbp" }).to_string();
    let body2 = json!({ "amount": 9999, "currency": "gbp" }).to_string();

    let res1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "conflict-key")
                .body(Body::from(body1))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::CREATED);

    let res2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "conflict-key")
                .body(Body::from(body2))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "./migrations")]
async fn idempotency_reconstructs_response_if_response_body_missing(pool: PgPool) {
    let app = build_app(AppState { db: pool.clone() });

    let pi_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO payment_intents (id, amount, currency, status)
        VALUES ($1, $2, $3, $4)
        "#,
        pi_id,
        2500_i64,
        "gbp",
        "requires_confirmation"
    )
    .execute(&pool)
    .await
    .unwrap();

    // Insert idempotency row that looks like "reserved" but response_body not written
    let req_hash = "amount=2500&currency=gbp";

    sqlx::query!(
        r#"
        INSERT INTO idempotency_keys (key, endpoint, request_hash, response_body, payment_intent_id)
        VALUES ($1, $2, $3, '{}'::jsonb, $4)
        "#,
        "crash-window-key",
        "POST /v1/payment_intents",
        req_hash,
        pi_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Call the API as if retrying the same request
    let body = json!({ "amount": 2500, "currency": "gbp" }).to_string();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/payment_intents")
                .header("content-type", "application/json")
                .header("Idempotency-Key", "crash-window-key")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(v["id"].as_str().unwrap(), pi_id.to_string());
    assert_eq!(v["amount"], 2500);
    assert_eq!(v["currency"], "gbp");
    assert_eq!(v["status"], "requires_confirmation");
}

#[sqlx::test(migrations = "./migrations")]
async fn create_then_confirm_payment_intent_sets_succeeded(pool: PgPool) {
    let app = build_app(AppState { db: pool });

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
    let id = created["id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/payment_intents/{id}/confirm"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let confirmed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(confirmed["id"], created["id"]);
    assert_eq!(confirmed["status"], "succeeded");

    // Get again to ensure DB updated
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
    assert_eq!(fetched["status"], "succeeded");
}

#[sqlx::test(migrations = "./migrations")]
async fn confirming_twice_returns_409(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let body = json!({ "amount": 1500, "currency": "gbp" }).to_string();
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
    let id = created["id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/payment_intents/{id}/confirm"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Second confirm should conflict
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/payment_intents/{id}/confirm"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "./migrations")]
async fn confirm_unknown_payment_intent_returns_404(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let random_id = Uuid::new_v4();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/payment_intents/{random_id}/confirm"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_payment_intent_writes_created_outbox_event(pool: PgPool) {
    let app = build_app(AppState { db: pool.clone() });

    // Create payment intent via API
    let body = json!({ "amount": 2000, "currency": "gbp" }).to_string();

    let res = app
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
    let pi_id = created["id"].as_str().unwrap();

    // Assert outbox event written
    let row = sqlx::query!(
        r#"
        SELECT event_type, payload
        FROM events_outbox
        ORDER BY created_at DESC
        LIMIT 1
        "#
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.event_type, "payment_intent.created");
    assert_eq!(row.payload["payment_intent"]["id"].as_str().unwrap(), pi_id);
    assert_eq!(row.payload["payment_intent"]["amount"], 2000);
    assert_eq!(row.payload["payment_intent"]["currency"], "gbp");
    assert_eq!(
        row.payload["payment_intent"]["status"].as_str().unwrap(),
        "requires_confirmation"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn confirm_payment_intent_writes_succeeded_outbox_event(pool: PgPool) {
    let app = build_app(AppState { db: pool.clone() });

    // Create via API
    let body = json!({ "amount": 3000, "currency": "gbp" }).to_string();

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
    let pi_id = created["id"].as_str().unwrap().to_string();

    // Confirm via API
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/payment_intents/{pi_id}/confirm"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // Assert a succeeded event exists for this payment intent
    let rows = sqlx::query!(
        r#"
        SELECT event_type, payload
        FROM events_outbox
        ORDER BY created_at ASC
        "#
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let succeeded_event = rows.iter().find(|row| {
        row.event_type == "payment_intent.succeeded"
            && row.payload["payment_intent"]["id"].as_str() == Some(pi_id.as_str())
    });

    let event = succeeded_event.expect("expected payment_intent.succeeded outbox event");

    assert_eq!(event.event_type, "payment_intent.succeeded");
    assert_eq!(event.payload["payment_intent"]["amount"], 3000);
    assert_eq!(event.payload["payment_intent"]["currency"], "gbp");
    assert_eq!(
        event.payload["payment_intent"]["status"].as_str().unwrap(),
        "succeeded"
    );
}
