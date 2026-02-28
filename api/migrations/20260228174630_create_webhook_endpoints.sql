CREATE TABLE webhook_endpoints (
  id UUID PRIMARY KEY,
  url TEXT NOT NULL,
  secret TEXT NOT NULL,
  is_enabled BOOLEAN NOT NULL DEFAULT true,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX webhook_endpoints_enabled_idx ON webhook_endpoints (is_enabled);
CREATE INDEX webhook_endpoints_created_at_idx ON webhook_endpoints (created_at);