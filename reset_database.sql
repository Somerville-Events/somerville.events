\set ON_ERROR_STOP on


-- pull directly from environment into psql variables
\getenv DB_NAME DB_NAME
\getenv DB_APP_USER DB_APP_USER
\getenv DB_APP_USER_PASS DB_APP_USER_PASS
\getenv DB_MIGRATOR DB_MIGRATOR
\getenv DB_MIGRATOR_PASS DB_MIGRATOR_PASS

-------------------------------------------------------
-- Teardown section (drop old DB and roles if they exist)
-------------------------------------------------------

DROP DATABASE IF EXISTS :"DB_NAME";

-- drop roles (order matters because of dependencies)
DROP ROLE IF EXISTS :"DB_APP_USER";
DROP ROLE IF EXISTS :"DB_MIGRATOR";
DROP ROLE IF EXISTS app_owner;

-------------------------------------------------------
-- Setup section
-------------------------------------------------------

-- roles
CREATE ROLE app_owner NOLOGIN;
CREATE ROLE :"DB_MIGRATOR" LOGIN PASSWORD :'DB_MIGRATOR_PASS';
CREATE ROLE :"DB_APP_USER" LOGIN PASSWORD :'DB_APP_USER_PASS';
GRANT app_owner TO :"DB_MIGRATOR";

-- database
CREATE DATABASE :"DB_NAME" OWNER app_owner;

\connect :"DB_NAME"

-- schema
CREATE SCHEMA app AUTHORIZATION app_owner;

-- lock down and grant explicit access
REVOKE CONNECT ON DATABASE :"DB_NAME" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"DB_NAME" TO :"DB_APP_USER", :"DB_MIGRATOR";

-- lock down public schema
REVOKE ALL ON SCHEMA public FROM PUBLIC;

-- allow roles to use app schema
GRANT USAGE ON SCHEMA app TO :"DB_APP_USER";
GRANT USAGE ON SCHEMA app TO :"DB_MIGRATOR";

-- privileges for app_user
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA app TO :"DB_APP_USER";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA app TO :"DB_APP_USER";

-- ensure future objects get the right privileges
SET ROLE app_owner;

ALTER DEFAULT PRIVILEGES IN SCHEMA app
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"DB_APP_USER";

ALTER DEFAULT PRIVILEGES IN SCHEMA app
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO :"DB_APP_USER";

RESET ROLE;

-- ensure future objects created by migrator get the right privileges
ALTER DEFAULT PRIVILEGES FOR ROLE :"DB_MIGRATOR" IN SCHEMA app
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"DB_APP_USER";

ALTER DEFAULT PRIVILEGES FOR ROLE :"DB_MIGRATOR" IN SCHEMA app
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO :"DB_APP_USER";
