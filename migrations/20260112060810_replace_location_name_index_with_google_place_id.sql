-- Drop the old index on location_name
DROP INDEX IF EXISTS app.idx_events_location_name;

-- Create a new index on google_place_id
CREATE INDEX IF NOT EXISTS idx_events_google_place_id ON app.events (google_place_id);
