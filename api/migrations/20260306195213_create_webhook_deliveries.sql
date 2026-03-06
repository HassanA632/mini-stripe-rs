CREATE TABLE webhook_deliveries (
  id UUID PRIMARY KEY,
  event_id UUID NOT NULL REFERENCES events_outbox(id) ON DELETE CASCADE,
  webhook_endpoint_id UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  attempt_count INT NOT NULL DEFAULT 0,
  last_attempt_at TIMESTAMPTZ NULL,
  next_attempt_at TIMESTAMPTZ NULL,
  last_error TEXT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (event_id, webhook_endpoint_id)
);

CREATE INDEX webhook_deliveries_status_next_attempt_idx
  ON webhook_deliveries (status, next_attempt_at);

CREATE INDEX webhook_deliveries_event_idx
  ON webhook_deliveries (event_id);

CREATE INDEX webhook_deliveries_endpoint_idx
  ON webhook_deliveries (webhook_endpoint_id);