#!/usr/bin/env bash
set -euo pipefail

# Go to the directory this script lives in (repo root)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure .env exists
if [[ ! -f .env ]]; then
  echo "Error: .env file not found in $SCRIPT_DIR" >&2
  exit 1
fi

echo "Loading environment from .env..."

# Export everything from .env so psql \getenv can see it
# This respects quoting and avoids the xargs issues.
set -a
# shellcheck disable=SC1091
source .env
set +a

# Basic sanity checks for the DB-related vars your SQL script expects
: "${DB_NAME:?DB_NAME is not set in .env}"
: "${DB_APP_USER:?DB_APP_USER is not set in .env}"
: "${DB_APP_USER_PASS:?DB_APP_USER_PASS is not set in .env}"
: "${DB_MIGRATOR:?DB_MIGRATOR is not set in .env}"
: "${DB_MIGRATOR_PASS:?DB_MIGRATOR_PASS is not set in .env}"

# Allow overrides, but default to your existing setup
PSQL_BIN="${PSQL_BIN:-psql-17}"
DB_SUPERUSER="${DB_SUPERUSER:-$(whoami)}"
DB_SUPERDB="${DB_SUPERDB:-postgres}"

echo "Using psql binary: $PSQL_BIN"
echo "Connecting as superuser: $DB_SUPERUSER to database: $DB_SUPERDB"
echo "Running reset_database.sql..."

"$PSQL_BIN" \
  -U "$DB_SUPERUSER" \
  -d "$DB_SUPERDB" \
  -f reset_database.sql

echo "Done. Database '$DB_NAME' should be initialized."