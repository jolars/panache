#!/usr/bin/env bash

# Script to generate benchmark docs from deterministic JSON benchmark output.

set -euo pipefail

OUTPUT_FILE="docs/benchmarks.qmd"
JSON_FILE="benches/benchmark_results.json"

echo "Running benchmarks..."
PANACHE_BENCH_OUTPUT_JSON="$JSON_FILE" cargo bench --bench formatting --quiet

python3 - "$JSON_FILE" "$OUTPUT_FILE" <<'PY'
import json
import sys
from pathlib import Path

json_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
data = json.loads(json_path.read_text(encoding="utf-8"))
results = [r for r in data.get("results", []) if r.get("built_in_greedy_wrap", True)]

lines = [
    "---",
    'title: "Performance Benchmarks"',
    'description: "panache performance metrics on real-world documents"',
    "---",
    "",
    "## Benchmark Suite",
    "",
    "Benchmarks are run on real Quarto documents to measure realistic performance.",
    "",
    "### Methodology",
    "",
    "- Measures three phases: full pipeline (parse+format), parse only, format only",
    "- Reports average time per iteration and throughput (KB/s)",
    "- Source of truth: `benches/benchmark_results.json` (schema version "
    + str(data.get("schema_version", "unknown"))
    + ")",
    "",
    "## Results",
    "",
]

for r in results:
    lines.extend(
        [
            f"### {r['name']}",
            "",
            f"Document: `{r['document']}` ({r['size_bytes']} bytes, {r['line_count']} lines)  ",
            f"Iterations: {r['iterations']}",
            "",
            "| Metric | Avg time |",
            "| --- | ---: |",
            f"| Full pipeline | {r['full_avg_us'] / 1000.0:.2f} ms |",
            f"| Parse only | {r['parse_avg_us'] / 1000.0:.2f} ms |",
            f"| Format only | {r['format_avg_us'] / 1000.0:.2f} ms |",
            f"| Throughput | {r['throughput_kb_s']:.2f} KB/s |",
            "",
        ]
    )

lines.extend(
    [
        "## Reproducing",
        "",
        "```bash",
        "# Download test documents",
        "cd benches/documents && ./download.sh",
        "",
        "# Generate JSON + docs page",
        "./benches/generate_docs.sh",
        "```",
        "",
    ]
)

output_path.write_text("\n".join(lines), encoding="utf-8")
PY

echo "✅ Benchmark artifacts saved:"
echo "  - $JSON_FILE"
echo "  - $OUTPUT_FILE"
