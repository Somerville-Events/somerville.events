ALTER TABLE app.events ADD COLUMN original_location TEXT;
UPDATE app.events SET original_location = location;
