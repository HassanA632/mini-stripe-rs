# mini-stripe-rs

A Rust backend project I made that simulates a Stripe-style **Payment Intents** API.

It focuses on real backend patterns you would expect in industry: REST endpoints, Postgres persistence, database migrations, integration tests, idempotency for safe retries, and an event-driven design using the **outbox pattern** with reliable webhook delivery.

---

## Features

- Create and fetch payment intents (`POST` / `GET`)
- Confirm payment intents to simulate payment completion (`POST /confirm`)
- **Idempotent create** using `Idempotency-Key` to prevent duplicate intents on retries
- Crash-window hardening for idempotency (can reconstruct a response using stored `payment_intent_id`)
- **Events outbox** recording lifecycle events:
  - `payment_intent.created`
  - `payment_intent.succeeded`
- Webhook endpoints registry:
  - Register webhook URL (returns secret once)
  - List registered endpoints (does not expose secrets)
- Webhook delivery worker:
  - Polls DB and delivers events to webhook endpoints
  - Retries with backoff
  - Retry cap (marks deliveries `failed` after max attempts)
  - Marks outbox events as delivered when all deliveries are complete
  - Includes a signature header for payload verification

---

## How it works

High-level flow:

1. API writes PaymentIntent state into Postgres
2. API writes a lifecycle event into `events_outbox` (outbox pattern)
3. A worker process polls for undelivered events
4. Worker creates/claims `webhook_deliveries` per event + endpoint
5. Worker sends webhook HTTP POST with a signature header
6. Worker updates delivery status and retries failures up to a cap
7. Once deliveries are complete, worker sets `events_outbox.delivered_at`

---

## Tech stack

- Rust
- Axum (HTTP API)
- SQLx (Postgres access + migrations)
- Docker + Postgres
- Tokio (async runtime)
- Reqwest (webhook delivery)

---

## Requirements

- Rust + Cargo
- Docker (for Postgres)

---

## Run locally

Start Postgres:

```bash
docker compose up -d
```

Run migrations:

```bash
set -a; source .env; set +a
sqlx migrate run --source api/migrations
```

Run the API:

```bash
cargo run -p api
```

Run the worker:

```bash
set -a; source .env; set +a
cargo run -p worker
```

---

## API usage

Create a payment intent:

```bash
curl -i -X POST http://localhost:3000/v1/payment_intents \
  -H "content-type: application/json" \
  -d '{"amount":100,"currency":"gbp"}'
```

Create with idempotency (safe retries):

```bash
curl -i -X POST http://localhost:3000/v1/payment_intents \
  -H "content-type: application/json" \
  -H "Idempotency-Key: abc123" \
  -d '{"amount":100,"currency":"gbp"}'
```

Confirm (simulate payment success):

```bash
curl -i -X POST http://localhost:3000/v1/payment_intents/<ID>/confirm
```

Register a webhook endpoint (returns secret once):

```bash
curl -i -X POST http://localhost:3000/v1/webhook_endpoints \
  -H "content-type: application/json" \
  -d '{"url":"http://localhost:9000/webhook"}'
```

List webhook endpoints (no secrets):

```bash
curl -i http://localhost:3000/v1/webhook_endpoints
```

---

## Testing

Run all tests:

```bash
set -a; source .env; set +a
cargo test
```

Includes integration tests for:

- payment intent create/get/confirm
- idempotency semantics (including crash-window recovery)
- outbox events being recorded
- webhook endpoint registration/listing

---

## Notes / Limitations

- Payments are simulated so no real card network integration.
- Webhook delivery is designed for reliability (tracking + retries), but still intentionally lightweight so I can still learn as I develop without the scope getting out of hand.
