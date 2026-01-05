-- 1. Insert ImageUpload into source_names
INSERT INTO app.source_names (name) VALUES ('ImageUpload') ON CONFLICT DO NOTHING;

-- 2. Backfill NULL source_name with 'ImageUpload'
UPDATE app.events SET source_name = 'ImageUpload' WHERE source_name IS NULL;

-- 3. Make source_name NOT NULL
ALTER TABLE app.events ALTER COLUMN source_name SET NOT NULL;

-- 4. Rename column source_name to source
ALTER TABLE app.events RENAME COLUMN source_name TO source;
