#!/usr/bin/env bash
# Crash-replay backtest: numbers, results JSON and chart for docs/BACKTEST.md.
# Offline (committed CSVs only). Run from clearinghouse/.
set -euo pipefail
cd "$(dirname "$0")/../.."
npm --prefix services/risk ci --no-audit --no-fund
npm --prefix services/risk test
(cd services/risk && npx tsc && node dist/services/risk/src/cli.js backtest)
python -I scripts/backtest/chart.py docs/backtest/results.json docs/backtest/backtest.png
