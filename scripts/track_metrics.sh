#!/bin/bash
set -e

# Build release binary
echo "Building release binary..."
cargo build --release

# Measure binary size
BINARY_PATH="target/release/somerville-events"
if [[ "$OSTYPE" == "darwin"* ]]; then
    BINARY_SIZE=$(stat -f%z "$BINARY_PATH")
else
    BINARY_SIZE=$(stat -c%s "$BINARY_PATH")
fi
echo "Binary size: $BINARY_SIZE bytes"

# Count dependencies
echo "Counting dependencies..."
DEP_COUNT=$(cargo tree --prefix none | wc -l | tr -d ' ')
echo "Dependency count: $DEP_COUNT"

# Enforce dependency limit
DEP_LIMIT=300
if [ "$DEP_COUNT" -gt "$DEP_LIMIT" ]; then
    echo "Error: Dependency count $DEP_COUNT exceeds limit $DEP_LIMIT"
    exit 1
fi

# Generate output JSON
OUTPUT_FILE="metrics.json"
cat <<EOF > "$OUTPUT_FILE"
[
    {
        "name": "Binary Size",
        "unit": "bytes",
        "value": $BINARY_SIZE
    },
    {
        "name": "Dependency Count",
        "unit": "count",
        "value": $DEP_COUNT
    }
]
EOF

echo "Metrics saved to $OUTPUT_FILE"


