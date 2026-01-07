-- Enable trigram extension for fuzzy matching on name
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Index for fuzzy matching on name
CREATE INDEX IF NOT EXISTS idx_events_name_trgm ON app.events USING gin (name gin_trgm_ops);

-- Index for location_name filtering
CREATE INDEX IF NOT EXISTS idx_events_location_name ON app.events (location_name);

-- Index for price filtering (finding free events)
CREATE INDEX IF NOT EXISTS idx_events_price ON app.events (price);
