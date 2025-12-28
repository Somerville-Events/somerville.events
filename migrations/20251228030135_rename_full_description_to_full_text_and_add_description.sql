ALTER TABLE app.events RENAME COLUMN full_description TO full_text;
ALTER TABLE app.events ADD COLUMN description TEXT NOT NULL DEFAULT '';
