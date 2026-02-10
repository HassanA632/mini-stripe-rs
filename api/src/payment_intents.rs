use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct CreatePaymentIntentRequest {
    amount: i64,
    currency: String,
}

#[derive(Serialize)]
pub struct PaymentIntentResponse {
    id: Uuid,
    amount: i64,
    currency: String,
    status: String,
}

pub async fn create_payment_intent(
    State(state): State<AppState>,
    Json(req): Json<CreatePaymentIntentRequest>,
) -> Result<(StatusCode, Json<PaymentIntentResponse>), (StatusCode, String)> {
    if req.amount <= 0 {
        return Err((StatusCode::BAD_REQUEST, "amount must be > 0".to_string()));
    }
    if req.currency.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "currency is required".to_string()));
    }

    let id = Uuid::new_v4();
    let status = "requires_confirmation";

    sqlx::query!(
        r#"
        INSERT INTO payment_intents (id, amount, currency, status)
        VALUES ($1, $2, $3, $4)
        "#,
        id,
        req.amount,
        req.currency,
        status
    )
    .execute(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(PaymentIntentResponse {
            id,
            amount: req.amount,
            currency: req.currency,
            status: status.to_string(),
        }),
    ))
}

pub async fn get_payment_intent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PaymentIntentResponse>, (StatusCode, String)> {
    let row = sqlx::query!(
        r#"
        SELECT id, amount, currency, status
        FROM payment_intents
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    match row {
        Some(pi) => Ok(Json(PaymentIntentResponse {
            id: pi.id,
            amount: pi.amount,
            currency: pi.currency,
            status: pi.status,
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            "payment_intent not found".to_string(),
        )),
    }
}
