use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreateWebhookEndpointRequest {
    pub url: String,
}

#[derive(Serialize)]
pub struct WebhookEndpointCreatedResponse {
    pub id: Uuid,
    pub url: String,
    pub secret: String, // returns only on creation
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct WebhookEndpointListItem {
    pub id: Uuid,
    pub url: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
}

fn generate_secret() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

pub async fn create_webhook_endpoint(
    State(state): State<AppState>,
    Json(req): Json<CreateWebhookEndpointRequest>,
) -> Result<(StatusCode, Json<WebhookEndpointCreatedResponse>), (StatusCode, String)> {
    if req.url.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "url is required".to_string()));
    }

    let id = Uuid::new_v4();
    let secret = generate_secret();

    let row = sqlx::query!(
        r#"
        INSERT INTO webhook_endpoints (id, url, secret)
        VALUES ($1, $2, $3)
        RETURNING id, url, secret, is_enabled, created_at
        "#,
        id,
        req.url,
        secret
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(WebhookEndpointCreatedResponse {
            id: row.id,
            url: row.url,
            secret: row.secret,
            is_enabled: row.is_enabled,
            created_at: row.created_at,
        }),
    ))
}

pub async fn list_webhook_endpoints(
    State(state): State<AppState>,
) -> Result<Json<Vec<WebhookEndpointListItem>>, (StatusCode, String)> {
    let rows = sqlx::query!(
        r#"
        SELECT id, url, is_enabled, created_at
        FROM webhook_endpoints
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    let items = rows
        .into_iter()
        .map(|r| WebhookEndpointListItem {
            id: r.id,
            url: r.url,
            is_enabled: r.is_enabled,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(items))
}
