CREATE TABLE app.idempotency_keys (
    idempotency_key UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
