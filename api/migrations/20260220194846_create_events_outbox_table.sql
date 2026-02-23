CREATE TABLE events_outbox (
  id UUID PRIMARY KEY,
  event_type TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  delivered_at TIMESTAMPTZ NULL
);

CREATE INDEX events_outbox_created_at_idx ON events_outbox (created_at);
CREATE INDEX events_outbox_delivered_at_idx ON events_outbox (delivered_at);