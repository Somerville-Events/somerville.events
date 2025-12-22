-- Remove events without a start date; they are not usable.
DELETE FROM app.events WHERE start_date IS NULL;

-- Enforce start_date presence going forward.
ALTER TABLE app.events
    ALTER COLUMN start_date SET NOT NULL;
