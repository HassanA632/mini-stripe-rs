ALTER TABLE idempotency_keys
ADD COLUMN payment_intent_id UUID NULL;

ALTER TABLE idempotency_keys
ADD CONSTRAINT idempotency_keys_payment_intent_fk
FOREIGN KEY (payment_intent_id) REFERENCES payment_intents(id);
