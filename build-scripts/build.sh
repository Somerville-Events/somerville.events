#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="somerville-events"
SHA="${1:?need sha}"
REPO="$HOME/repo.git"
ARTIFACTS="$HOME/artifacts"

# Ensure PATH includes rust toolchain for non-login shells
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:$PATH"

# Make a clean worktree for the SHA (idempotent)
WORKTREE="$HOME/worktrees/${SHA}"
if [ -d "$WORKTREE" ]; then
  git --git-dir="$REPO" worktree remove --force "$WORKTREE" || true
fi
git --git-dir="$REPO" worktree add --force --detach "$WORKTREE" "$SHA"


cd "$WORKTREE"

# Ensure database schema is up-to-date
source "$HOME/.env"
DATABASE_URL="postgresql://$DB_MIGRATOR:$DB_MIGRATOR_PASS@localhost/$DB_NAME"
sqlx database create --database-url "$DATABASE_URL"
sqlx migrate run --database-url "$DATABASE_URL"

# Build release
cargo build --release

# Store binary as git SHA and symlink it as the deployed binary
DEST="${ARTIFACTS}/${SHA}"
cp -f "target/release/${APP_NAME}" "$DEST"
ln -sfn "$DEST" "$HOME/bin/${APP_NAME}"

# Handle static files
STATIC_DEST="${ARTIFACTS}/${SHA}-static"
rm -rf "$STATIC_DEST"
cp -r "static" "$STATIC_DEST"
mkdir -p "$HOME/srv"
ln -sfn "$STATIC_DEST" "$HOME/srv/static"

systemctl --user restart "${APP_NAME}.service"

# Prune old worktrees and keep last N artifacts
git --git-dir="$REPO" worktree prune || true
( cd "$ARTIFACTS" && ls -1t | tail -n +20 | xargs -r rm -rf )