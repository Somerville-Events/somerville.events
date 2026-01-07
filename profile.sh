#!/usr/bin/env bash
set -euo pipefail

# Configuration
PROFILE_OUTPUT="profile.json"
APP_BIN="./target/release/somerville-events"
URL="http://127.0.0.1:8080/"
REQUESTS=2000
CONCURRENCY=20

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

log() {
    echo -e "${GREEN}[PROFILE]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# 1. Check prerequisites
if ! command -v samply &> /dev/null; then
    error "samply is not installed. Run 'brew install samply' or 'cargo install samply'."
fi

if ! command -v ab &> /dev/null; then
    error "ab (Apache Bench) is not installed."
fi

# 2. Build release mode
log "Building in release mode..."
cargo build --release || error "Build failed"

# 3. Cleanup existing processes
log "Cleaning up existing processes..."
# Find PID listening on 8080 or matching binary name and kill it
# This is a bit aggressive but ensures we can bind the port
if pgrep -f "somerville-events" > /dev/null; then
    pkill -f "somerville-events" || true
    sleep 2
fi

# 4. Start Profiler
log "Starting application with samply..."
# Remove old profile if exists
rm -f "$PROFILE_OUTPUT"

# Run samply in background
# --save-only prevents it from opening the browser immediately
samply record --save-only -o "$PROFILE_OUTPUT" -- "$APP_BIN" &
SAMPLY_PID=$!

# 5. Wait for server to be ready
log "Waiting for server to start..."
MAX_RETRIES=30
count=0
while ! curl -s "$URL" > /dev/null; do
    sleep 0.5
    count=$((count+1))
    if [ $count -ge $MAX_RETRIES ]; then
        kill "$SAMPLY_PID" || true
        error "Server failed to start within timeout."
    fi
done

# 6. Generate Load
log "Generating load ($REQUESTS requests, $CONCURRENCY concurrency)..."
ab -n "$REQUESTS" -c "$CONCURRENCY" "$URL"

# 7. Stop Profiler
log "Stopping profiler..."
kill -INT "$SAMPLY_PID"
wait "$SAMPLY_PID" || true

log "Profiling complete!"
log "Results saved to: $PROFILE_OUTPUT"
log "To view results, run: samply load $PROFILE_OUTPUT"

