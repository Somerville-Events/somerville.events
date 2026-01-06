-- Drop old indices that might reference the dropped event_type column
DROP INDEX IF EXISTS app.idx_events_type_date;

-- Index for filtering by source and sorting by start_date
CREATE INDEX IF NOT EXISTS idx_events_source_start_date ON app.events (source, start_date);

-- Index for the event_event_types join table to support filtering by event type
-- We need to find events given an event type
CREATE INDEX IF NOT EXISTS idx_event_event_types_type_name_event_id ON app.event_event_types (event_type_name, event_id);

-- Index to speed up duplicate detection
-- Used in find_duplicate: WHERE start_date = $1 AND end_date IS NOT DISTINCT FROM $2 AND address IS NOT DISTINCT FROM $3
CREATE INDEX IF NOT EXISTS idx_events_duplicates ON app.events (start_date, end_date, address);
