\set ON_ERROR_STOP on


-- pull directly from environment into psql variables
\getenv DB_NAME DB_NAME
\getenv DB_APP_USER_PASS DB_APP_USER_PASS
\getenv DB_MIGRATOR_PASS DB_MIGRATOR_PASS

-------------------------------------------------------
-- Teardown section (drop old DB and roles if they exist)
-------------------------------------------------------

DROP DATABASE IF EXISTS :"DB_NAME";

-- drop roles (order matters because of dependencies)
DROP ROLE IF EXISTS app_user;
DROP ROLE IF EXISTS migrator;
DROP ROLE IF EXISTS app_owner;

-------------------------------------------------------
-- Setup section
-------------------------------------------------------

-- roles
CREATE ROLE app_owner NOLOGIN;
CREATE ROLE migrator LOGIN PASSWORD :'DB_MIGRATOR_PASS' CREATEDB;
CREATE ROLE app_user LOGIN PASSWORD :'DB_APP_USER_PASS';
GRANT app_owner TO migrator;

-- database
CREATE DATABASE :"DB_NAME" OWNER app_owner;

\connect :"DB_NAME"

-- lock down and grant explicit access
REVOKE CONNECT ON DATABASE :"DB_NAME" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"DB_NAME" TO app_user, migrator;

-- lock down public schema
REVOKE ALL ON SCHEMA public FROM PUBLIC;
