#!/usr/bin/env bash
# End-to-end demo: serves two small fictional job pages locally and runs job_radar on them
# with a small local Ollama model. No API key and no internet access to job sites needed.
# The demo does not use --kill-dups: a model this small does not merge duplicates reliably.
#
# Needs: cargo, python3, and Ollama running with the model from demo/config.yaml:
#   ollama pull qwen3:1.7b
#   demo/run.sh
set -euo pipefail
cd "$(dirname "$0")/.."

PORT=8765
OUT=demo/result.csv

python3 -m http.server "$PORT" --bind 127.0.0.1 --directory demo/site >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  curl -fs "http://127.0.0.1:$PORT/jobboard.html" >/dev/null && break
  sleep 0.1
done

AGENTS_CONFIG=demo/config.yaml \
JOB_RADAR_SOURCES=demo/sources.yaml \
JOB_RADAR_IGNORE=demo/ignore.yaml \
  cargo run --release --quiet -- --cv demo/demo_cv.txt --out "$OUT"
