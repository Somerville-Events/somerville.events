CREATE INDEX idx_events_start_date ON app.events (start_date);
CREATE INDEX idx_events_type_date ON app.events (event_type, start_date);
