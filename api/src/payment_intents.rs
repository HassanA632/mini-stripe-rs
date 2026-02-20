use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events_outbox::insert_event;
use crate::state::AppState;

const IDEMPOTENCY_ENDPOINT: &str = "POST /v1/payment_intents";

#[derive(Deserialize)]
pub struct CreatePaymentIntentRequest {
    amount: i64,
    currency: String,
}

#[derive(Serialize, Deserialize)]
pub struct PaymentIntentResponse {
    id: Uuid,
    amount: i64,
    currency: String,
    status: String,
}

fn request_fingerprint(req: &CreatePaymentIntentRequest) -> String {
    format!(
        "amount={}&currency={}",
        req.amount,
        req.currency.trim().to_lowercase()
    )
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
    headers: HeaderMap,
    Json(req): Json<CreatePaymentIntentRequest>,
) -> Result<(StatusCode, Json<PaymentIntentResponse>), (StatusCode, String)> {
    if let Err(msg) = validate_create_payment_intent(&req) {
        return Err((StatusCode::BAD_REQUEST, msg.to_string()));
    }

    // Read header
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // If no idempotency key keep current behavior
    if idempotency_key.is_none() {
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

        return Ok((
            StatusCode::CREATED,
            Json(PaymentIntentResponse {
                id,
                amount: req.amount,
                currency: req.currency,
                status: status.to_string(),
            }),
        ));
    }

    // Idempotent path
    let key = idempotency_key.unwrap();
    let req_hash = request_fingerprint(&req);

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    // Reserve the key if its new
    // If already used this returns 0 rows
    let reserved = sqlx::query!(
        r#"
        INSERT INTO idempotency_keys (key, endpoint, request_hash, response_body)
        VALUES ($1, $2, $3, '{}'::jsonb)
        ON CONFLICT (key, endpoint) DO NOTHING
        RETURNING key
        "#,
        key,
        IDEMPOTENCY_ENDPOINT,
        req_hash
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    if reserved.is_some() {
        // Successfully reserved the key -> create payment intent
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
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        let response = PaymentIntentResponse {
            id,
            amount: req.amount,
            currency: req.currency,
            status: status.to_string(),
        };

        // Store the response JSON so retries can return the same thing
        let response_json = serde_json::to_value(&response).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("json error: {e}"),
            )
        })?;

        // Server Crash Edge Case: we store payment_intent_id as well as response_body.
        // If the server crashes after reserving the idempotency key but before writing
        // the final response_body: retries can reconstruct the response from payment_intents.
        sqlx::query!(
            r#"
            UPDATE idempotency_keys
            SET response_body = $1, payment_intent_id = $2
            WHERE key = $3 AND endpoint = $4
            "#,
            response_json,
            id,
            key,
            IDEMPOTENCY_ENDPOINT
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        // Outbox event to record that a new payment intent was created
        let payload = serde_json::json!({
            "payment_intent": {
                "id": response.id,
                "amount": response.amount,
                "currency": response.currency.clone(),
                "status": response.status.clone()
            }
        });

        insert_event(&mut *tx, "payment_intent.created", payload)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        return Ok((StatusCode::CREATED, Json(response)));
    }

    // Key already exists = fetch stored record
    let row = sqlx::query!(
        r#"
    SELECT request_hash, response_body, payment_intent_id
    FROM idempotency_keys
    WHERE key = $1 AND endpoint = $2
    "#,
        key,
        IDEMPOTENCY_ENDPOINT
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    // If request differs its a conflict
    if row.request_hash != req_hash {
        tx.rollback().await.ok();
        return Err((
            StatusCode::CONFLICT,
            "idempotency key reused with different request".to_string(),
        ));
    }

    // If response_body looks complete return it
    let looks_complete = row
        .response_body
        .get("id")
        .and_then(|v| v.as_str())
        .is_some();

    if looks_complete {
        let response: PaymentIntentResponse =
            serde_json::from_value(row.response_body).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("json error: {e}"),
                )
            })?;

        tx.commit().await.ok();
        return Ok((StatusCode::CREATED, Json(response)));
    }

    // Crash fallback: response_body is incomplete: reconstruct using payment_intent_id
    if let Some(pi_id) = row.payment_intent_id {
        let pi = sqlx::query!(
            r#"
        SELECT id, amount, currency, status
        FROM payment_intents
        WHERE id = $1
        "#,
            pi_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        let response = PaymentIntentResponse {
            id: pi.id,
            amount: pi.amount,
            currency: pi.currency,
            status: pi.status,
        };

        // fill response_body so future retries are fast
        let response_json = serde_json::to_value(&response).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("json error: {e}"),
            )
        })?;

        sqlx::query!(
            r#"
        UPDATE idempotency_keys
        SET response_body = $1
        WHERE key = $2 AND endpoint = $3
        "#,
            response_json,
            key,
            IDEMPOTENCY_ENDPOINT
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        tx.commit().await.ok();
        return Ok((StatusCode::CREATED, Json(response)));
    }

    // Idempotency record exists but is incomplete in a way we cant recover from
    tx.rollback().await.ok();
    return Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        "idempotency record exists but has no stored response or payment_intent_id".to_string(),
    ));
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

pub async fn confirm_payment_intent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PaymentIntentResponse>, (StatusCode, String)> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    // Try to update only if in the correct state
    let updated = sqlx::query!(
        r#"
        UPDATE payment_intents
        SET status = 'succeeded'
        WHERE id = $1 AND status = 'requires_confirmation'
        RETURNING id, amount, currency, status
        "#,
        id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    if let Some(pi) = updated {
        let response = PaymentIntentResponse {
            id: pi.id,
            amount: pi.amount,
            currency: pi.currency,
            status: pi.status,
        };

        // Outbox event records successful confirmation
        let payload = serde_json::json!({
            "payment_intent": {
                "id": response.id,
                "amount": response.amount,
                "currency": response.currency.clone(),
                "status": response.status.clone()
            }
        });

        insert_event(&mut *tx, "payment_intent.succeeded", payload)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

        return Ok(Json(response));
    }

    // Not updated = not found/invalid state
    let exists = sqlx::query!(
        r#"
        SELECT status
        FROM payment_intents
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}")))?;

    // No state change happened safe to rollback
    tx.rollback().await.ok();

    match exists {
        None => Err((
            StatusCode::NOT_FOUND,
            "payment_intent not found".to_string(),
        )),
        Some(row) => Err((
            StatusCode::CONFLICT,
            format!("cannot confirm payment_intent in status '{}'", row.status),
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
