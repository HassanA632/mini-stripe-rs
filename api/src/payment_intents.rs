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

fn validate_create_payment_intent(req: &CreatePaymentIntentRequest) -> Result<(), &'static str> {
    if req.amount <= 0 {
        return Err("amount must be > 0");
    }
    if req.currency.trim().is_empty() {
        return Err("currency is required");
    }
    Ok(())
}

pub async fn create_payment_intent(
    State(state): State<AppState>,
    Json(req): Json<CreatePaymentIntentRequest>,
) -> Result<(StatusCode, Json<PaymentIntentResponse>), (StatusCode, String)> {
    if let Err(msg) = validate_create_payment_intent(&req) {
        return Err((StatusCode::BAD_REQUEST, msg.to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_non_positive_amount() {
        let req = CreatePaymentIntentRequest {
            amount: 0,
            currency: "gbp".to_string(),
        };

        let err = validate_create_payment_intent(&req).unwrap_err();
        assert_eq!(err, "amount must be > 0");
    }

    #[test]
    fn validate_rejects_empty_currency() {
        let req = CreatePaymentIntentRequest {
            amount: 2500,
            currency: "   ".to_string(),
        };

        let err = validate_create_payment_intent(&req).unwrap_err();
        assert_eq!(err, "currency is required");
    }

    #[test]
    fn validate_accepts_good_input() {
        let req = CreatePaymentIntentRequest {
            amount: 2500,
            currency: "gbp".to_string(),
        };

        assert!(validate_create_payment_intent(&req).is_ok());
    }
}
