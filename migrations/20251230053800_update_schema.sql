-- 1. Create table for event types
CREATE TABLE app.event_types (
    name TEXT PRIMARY KEY
);

-- Populate event types from the enum
INSERT INTO app.event_types (name) VALUES
('YardSale'),
('Art'),
('Music'),
('Dance'),
('Performance'),
('Food'),
('PersonalService'),
('Meeting'),
('Government'),
('Volunteer'),
('Fundraiser'),
('Film'),
('Theater'),
('Comedy'),
('Literature'),
('Exhibition'),
('Workshop'),
('Fitness'),
('Market'),
('Sports'),
('Family'),
('Social'),
('Holiday'),
('Religious'),
('Other');

-- Add new child friendly event type
INSERT INTO app.event_types (name) VALUES ('ChildFriendly');

-- 2. Create join table for event types
CREATE TABLE app.event_event_types (
    event_id BIGINT NOT NULL REFERENCES app.events(id) ON DELETE CASCADE,
    event_type_name TEXT NOT NULL REFERENCES app.event_types(name) ON DELETE CASCADE,
    PRIMARY KEY (event_id, event_type_name)
);

-- Backfill data from events.event_type
INSERT INTO app.event_event_types (event_id, event_type_name)
SELECT id, event_type::text
FROM app.events
WHERE event_type IS NOT NULL;

-- 3. Add age_restrictions
ALTER TABLE app.events ADD COLUMN age_restrictions TEXT NULL;

-- 4. Add price
ALTER TABLE app.events ADD COLUMN price DOUBLE PRECISION NULL;

-- 5. Add updated_at
ALTER TABLE app.events ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
-- Backfill updated_at to created_at
UPDATE app.events SET updated_at = created_at;

-- Trigger to update updated_at
CREATE OR REPLACE FUNCTION app.update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
   NEW.updated_at = now();
   RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_events_updated_at
BEFORE UPDATE ON app.events
FOR EACH ROW
EXECUTE PROCEDURE app.update_updated_at_column();

-- 6. Add source name
CREATE TABLE app.source_names (
    name TEXT PRIMARY KEY
);

INSERT INTO app.source_names (name) VALUES
('Aeronaut Brewing'),
('American Repertory Theater'),
('Arts at the Armory'),
('Boston Swing Central'),
('BostonShows.org'),
('Brattle Theatre'),
('Central Square Theater'),
('City of Cambridge'),
('Harvard Art Museums'),
('Harvard Book Store'),
('Lamplighter Brewing'),
('Porter Square Books'),
('Portico Brewing'),
('Sanders Theatre'),
('Somerville Theatre'),
('The Comedy Studio'),
('The Dance Complex'),
('The Lily Pad'),
('The Middle East');

ALTER TABLE app.events ADD COLUMN source_name TEXT REFERENCES app.source_names(name);

-- 7. Drop old event_type column and type
ALTER TABLE app.events DROP COLUMN event_type;
DROP TYPE app.event_type;
