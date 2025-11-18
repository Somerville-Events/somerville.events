#!/usr/bin/env bash
set -euo pipefail

# Go to the directory this script lives in (repo root)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
  cat <<EOF
Usage: $(basename "$0") [ /path/to/.env ]

Reset the database. Optionally provide the path to a .env file as the
first positional argument. If supplied, the script will load environment
variables from that file. If not supplied, the script will use the
environment variables already present in the environment when the
script was invoked.

This script will change to its own directory (where reset_database.sql lives)
before running the SQL.

Options:
  -h, --help    Show this help message and exit
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

# If an argument is provided, treat it as the .env file to load. Otherwise
# fall back to using the current process environment (do not attempt to
# source a .env file from the script directory).
if [[ $# -ge 1 ]]; then
  ENV_FILE="$1"
  if [[ ! -f "$ENV_FILE" ]]; then
    echo "Error: .env file not found at '$ENV_FILE'" >&2
    exit 1
  fi

  echo "Loading environment from $ENV_FILE..."

  # Export everything from the provided .env so psql \getenv can see it
  # This respects quoting and avoids the xargs issues.
  set -a
  source "$ENV_FILE"
  set +a
else
  echo "No .env file provided; using existing environment variables." >&2
fi

# Basic sanity checks for the DB-related vars your SQL script expects
: "${DB_NAME:?DB_NAME is not set}"
: "${DB_APP_USER:?DB_APP_USER is not set}"
: "${DB_APP_USER_PASS:?DB_APP_USER_PASS is not set}"
: "${DB_MIGRATOR:?DB_MIGRATOR is not set}"
: "${DB_MIGRATOR_PASS:?DB_MIGRATOR_PASS is not set}"

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