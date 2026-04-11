#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"

npx esbuild "$CRATE_DIR/ts/chart.ts" \
  --bundle \
  --outfile="$CRATE_DIR/js/chart.js" \
  --format=iife \
  --external:d3 \
  --external:d3-scale \
  --external:d3-axis \
  --external:d3-zoom \
  --external:d3-selection \
  --external:d3-time \
  --external:d3-time-format \
  --external:d3-transition \
  --external:d3-interpolate \
  --external:d3-color \
  --external:d3-dispatch \
  --external:d3-ease \
  --external:d3-timer \
  --external:d3-format \
  --external:d3-array \
  --target=es2020
