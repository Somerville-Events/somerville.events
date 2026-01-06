-- Create schema owned by app_owner (migrator is a member of app_owner)
-- We use AUTHORIZATION to ensure ownership is correct even if run by migrator
CREATE SCHEMA IF NOT EXISTS app AUTHORIZATION app_owner;

-- Grants for app_user (application runtime user)
GRANT USAGE ON SCHEMA app TO app_user;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA app TO app_user;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA app TO app_user;

-- Ensure future objects created by app_owner get the right privileges
ALTER DEFAULT PRIVILEGES FOR ROLE app_owner IN SCHEMA app
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_user;

ALTER DEFAULT PRIVILEGES FOR ROLE app_owner IN SCHEMA app
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO app_user;

-- Also ensure objects created by migrator (if any accidentally bypass app_owner) are accessible
ALTER DEFAULT PRIVILEGES FOR ROLE migrator IN SCHEMA app
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO app_user;

ALTER DEFAULT PRIVILEGES FOR ROLE migrator IN SCHEMA app
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO app_user;

-- Create events table
CREATE TABLE app.events (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    full_description TEXT NOT NULL,
    start_date TIMESTAMPTZ NULL,
    end_date TIMESTAMPTZ NULL,
    location TEXT NULL,
    event_type TEXT NULL,
    additional_details TEXT [] NULL,
    confidence DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);