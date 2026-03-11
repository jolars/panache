#!/usr/bin/env bash

set -euo pipefail

./benches/generate_docs.sh >/dev/null

if git --no-pager diff --quiet -- benches/benchmark_results.json docs/benchmarks.qmd; then
    echo "✅ Benchmark docs are up to date."
    exit 0
fi

echo "❌ Benchmark docs are stale. Run ./benches/generate_docs.sh and commit updates."
git --no-pager diff -- benches/benchmark_results.json docs/benchmarks.qmd
exit 1
