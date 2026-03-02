use api::{app::build_app, state::AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

#[sqlx::test(migrations = "./migrations")]
async fn create_and_list_webhook_endpoints(pool: PgPool) {
    let app = build_app(AppState { db: pool });

    let body = json!({ "url": "https://example.com/webhooks" }).to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhook_endpoints")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(created["url"], "https://example.com/webhooks");
    assert!(created["id"].as_str().is_some());
    assert!(created["secret"].as_str().is_some());

    // List endpoints and shouldn't include secret
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/webhook_endpoints")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert!(list.is_array());
    let first = &list[0];

    assert_eq!(first["url"], "https://example.com/webhooks");
    assert!(first.get("secret").is_none());
}
