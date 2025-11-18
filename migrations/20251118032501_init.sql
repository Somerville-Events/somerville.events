-- Create events table
CREATE TABLE app.events (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    full_description TEXT NOT NULL,
    start_date TIMESTAMPTZ NULL,
    end_date TIMESTAMPTZ NULL,
    location TEXT NULL,
    event_type TEXT NULL,
    additional_details TEXT [] NULL,
    confidence DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);