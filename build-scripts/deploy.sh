#!/usr/bin/env bash
set -Eeuo pipefail

# This script is intended to be run on the VPS to deploy a pre-built binary.
# It assumes the binary and static files have been uploaded to a temporary location.

APP_NAME="somerville-events"
# Arguments: $1 = path to uploaded binary (e.g. ~/artifacts/incoming/somerville-events)
#            $2 = path to uploaded static dir (e.g. ~/artifacts/incoming/static)
#            $3 = SHA/Version identifier
BINARY_SRC="${1:?need binary path}"
STATIC_SRC="${2:?need static dir path}"
SHA="${3:?need sha}"

ARTIFACTS="$HOME/artifacts"
DEST="${ARTIFACTS}/${SHA}"
STATIC_DEST="${ARTIFACTS}/${SHA}-static"

# Ensure PATH includes rust toolchain for sqlx (if needed) and other tools
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"

echo "Deploying version $SHA..."

# 1. Run migrations
# We assume .env is in $HOME and has DATABASE_URL
if [ -f "$HOME/.env" ]; then
    source "$HOME/.env"
    
    # Fallback if not set in .env but available in environment
    if [ -z "${DATABASE_URL:-}" ] && [ -n "${DB_MIGRATOR:-}" ]; then
        DATABASE_URL="postgresql://$DB_MIGRATOR:$DB_MIGRATOR_PASS@localhost/$DB_NAME"
    fi
    
    if [ -n "${DATABASE_URL:-}" ]; then
        echo "Running database migrations..."
        # Migrations are assumed to be in the current directory (repo root or uploaded artifacts)
        # We will attempt to run them from the current directory
        if [ -d "migrations" ]; then
             sqlx migrate run --database-url "$DATABASE_URL"
        elif [ -d "$HOME/repo.git" ]; then
             # If no local migrations folder, check if we can run it from a checkout?
             # For now, we will rely on migrations being present in the CWD (which is set by ssh command or defaults to home)
             echo "Warning: No 'migrations' directory found in current path. Skipping migration."
        fi
    else
        echo "Warning: DATABASE_URL not found. Skipping migrations."
    fi
fi

# 2. Move binary to artifacts storage
mkdir -p "$ARTIFACTS"
cp -f "$BINARY_SRC" "$DEST"
chmod +x "$DEST"

# 3. Move static files
rm -rf "$STATIC_DEST"
cp -r "$STATIC_SRC" "$STATIC_DEST"

# 4. Update Symlinks
mkdir -p "$HOME/bin"
ln -sfn "$DEST" "$HOME/bin/${APP_NAME}"

mkdir -p "$HOME/srv"
ln -sfn "$STATIC_DEST" "$HOME/srv/static"

# 5. Restart Service
echo "Restarting service..."
systemctl --user restart "${APP_NAME}.service"

# 6. Prune old artifacts (keep last 20)
echo "Pruning old artifacts..."
( cd "$ARTIFACTS" && ls -1t | grep -v "incoming" | tail -n +21 | xargs -r rm -rf )

echo "Deployment of $SHA complete."

