#!/usr/bin/env bash
set -Eeuo pipefail

# This script is intended to be run on the VPS to deploy a pre-built binary.
# It assumes the binary and static files have been uploaded to:
# ~/artifacts/${SHA}

APP_NAME="somerville-events"
SHA="${1:?need sha}"
ARTIFACTS="$HOME/artifacts"
LATEST="${ARTIFACTS}/${SHA}"

# Ensure PATH includes rust toolchain for sqlx migrations
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"

cd "$LATEST"

# 1. Run migrations
source "$HOME/.env"
DATABASE_URL="postgresql://$DB_MIGRATOR:$DB_MIGRATOR_PASS@localhost/$DB_NAME"
sqlx database create --database-url "$DATABASE_URL"
sqlx migrate run --database-url "$DATABASE_URL"

# 2. Update Symlinks
ln -sfn "$LATEST/${APP_NAME}" "$HOME/bin/${APP_NAME}"
ln -sfn "$LATEST/static" "$HOME/srv/static"

# 3. Restart Service
systemctl --user restart "${APP_NAME}.service"

# 4. Prune old artifacts (keep last 20)
( cd "$ARTIFACTS" && ls -1t | tail -n +20 | xargs -r rm -rf )

echo "Deployment of $SHA complete."
