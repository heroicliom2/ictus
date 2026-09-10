#!/usr/bin/env bash
# Phase 0 benchmark harness (docs/roadmap.md, bench/README.md). Runs every
# registered design adapter under bench/adapters/, records pass/fail and
# wall-clock time, and prints a summary table.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ADAPTERS_DIR="$SCRIPT_DIR/adapters"

pass=0
fail=0
fail_names=()

printf '%-20s %-6s %8s\n' "DESIGN" "RESULT" "TIME(s)"
printf '%-20s %-6s %8s\n' "------" "------" "-------"

shopt -s nullglob
for adapter in "$ADAPTERS_DIR"/*.sh; do
    name="$(basename "$adapter" .sh)"
    start=$(date +%s.%N)
    output="$("$adapter" 2>&1)"
    status=$?
    end=$(date +%s.%N)
    elapsed=$(awk -v s="$start" -v e="$end" 'BEGIN { printf "%.2f", e - s }')

    if [ "$status" -eq 0 ]; then
        result="PASS"
        pass=$((pass + 1))
    else
        result="FAIL"
        fail=$((fail + 1))
        fail_names+=("$name")
    fi

    printf '%-20s %-6s %8s\n' "$name" "$result" "$elapsed"
    if [ "$result" = "FAIL" ]; then
        printf -- '--- %s output ---\n%s\n------------------\n' "$name" "$output"
    fi
done

echo
echo "$pass passed, $fail failed"
if [ "$fail" -gt 0 ]; then
    printf 'Failed: %s\n' "${fail_names[*]}"
    exit 1
fi
