use axum::{
    Json, Router,
    extract::Path,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Pool, Postgres};

use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: Pool<Postgres>,
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct CreatePaymentIntentRequest {
    amount: i64,
    currency: String,
}

#[derive(Serialize)]
struct PaymentIntentResponse {
    id: Uuid,
    amount: i64,
    currency: String,
    status: String,
}

async fn create_payment_intent(
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

async fn get_payment_intent(
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

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");

    let db = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to Postgres");

    let state = AppState { db };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/payment_intents", post(create_payment_intent))
        .route("/v1/payment_intents/:id", get(get_payment_intent))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("API listening on http://localhost:3000");
    axum::serve(listener, app).await.expect("server error");
}
