CREATE TABLE payment_intents (
  id UUID PRIMARY KEY,
  amount BIGINT NOT NULL CHECK (amount > 0),
  currency TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
