DELETE FROM app.events WHERE start_date IS NULL;
ALTER TABLE app.events ALTER COLUMN start_date SET NOT NULL;
