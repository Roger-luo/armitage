#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"

npx esbuild "$CRATE_DIR/ts/chart.ts" \
  --bundle \
  --outfile="$CRATE_DIR/js/chart.js" \
  --format=iife \
  --external:echarts \
  --target=es2020
