#!/usr/bin/env bash
#
# check-unwrap.sh — Audit .unwrap() calls in non-test code paths across a set
# of "hot" crates where a panic translates to a 500 + dropped client connection.
#
# Invoked by the production-readiness pass (Sprint 84 Phase C). Prints a
# triage list to stdout, one hit per line in `file:line:snippet` format, and
# exits nonzero if any hits are found. Use the optional TRIAGE env var to
# write the list to a file as well.
#
# Usage:
#   scripts/check-unwrap.sh              # print hits, exit 1 if any
#   TRIAGE=unwraps.txt scripts/check-unwrap.sh
#
# What counts as a "hit":
#   - A `.unwrap()` call in a `.rs` file under `crates/<HOT_CRATE>/src/`
#   - Excluding lines that fall inside a `#[cfg(test)]` block
#   - Excluding lines in files named `tests.rs` (whole-file test modules
#     declared as `#[cfg(test)] mod tests;` in lib.rs)
#   - Excluding lines in files under `tests/` subdirectories
#   - Excluding lines in files named `*_tests.rs`
#
# Limitations: the inline test-block exclusion is heuristic. It walks the
# file once and flips a "in test block" flag at any `#[cfg(test)]`
# attribute, resetting at the end of the enclosing item (first top-level
# `}` at column 0). False positives and false negatives are both possible
# but rare in practice.

set -euo pipefail

HOT_CRATES=(
    "agentzero-gateway"
    "agentzero-orchestrator"
    "agentzero-infra"
    "agentzero-providers"
)

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

total=0
per_crate_counts=()

audit_file() {
    local file="$1"
    local in_test_block=0
    local line_num=0
    while IFS= read -r line; do
        line_num=$((line_num + 1))
        if [[ "$line" == *"#[cfg(test)]"* ]]; then
            in_test_block=1
            continue
        fi
        if [[ "$in_test_block" == "1" && "$line" == "}"* && "$line" != *"{"* ]]; then
            in_test_block=0
            continue
        fi
        if [[ "$in_test_block" == "1" ]]; then
            continue
        fi
        if [[ "$line" == *".unwrap()"* ]]; then
            # Skip commented-out unwraps
            trimmed="${line#"${line%%[![:space:]]*}"}"
            if [[ "$trimmed" == "//"* ]]; then
                continue
            fi
            echo "${file}:${line_num}:${trimmed}"
        fi
    done <"$file"
}

for crate in "${HOT_CRATES[@]}"; do
    src_dir="${REPO_ROOT}/crates/${crate}/src"
    if [[ ! -d "$src_dir" ]]; then
        echo "error: ${src_dir} does not exist" >&2
        exit 2
    fi

    count=0
    while IFS= read -r file; do
        base="$(basename "$file")"
        # Skip whole-file test modules: `tests.rs`, `*_tests.rs`, anything
        # under a `tests/` subdirectory.
        if [[ "$base" == "tests.rs" || "$base" == *_tests.rs ]]; then
            continue
        fi
        if [[ "$file" == */tests/* ]]; then
            continue
        fi
        hits=$(audit_file "$file" || true)
        if [[ -n "$hits" ]]; then
            echo "$hits"
            file_hits=$(echo "$hits" | wc -l | tr -d ' ')
            count=$((count + file_hits))
        fi
    done < <(find "$src_dir" -type f -name "*.rs")

    per_crate_counts+=("${crate}=${count}")
    total=$((total + count))
done

echo "" >&2
echo "=== Summary ===" >&2
for entry in "${per_crate_counts[@]}"; do
    echo "  ${entry}" >&2
done
echo "  TOTAL=${total}" >&2

if [[ -n "${TRIAGE:-}" ]]; then
    {
        echo "=== Summary ==="
        for entry in "${per_crate_counts[@]}"; do
            echo "  ${entry}"
        done
        echo "  TOTAL=${total}"
    } >>"${TRIAGE}"
fi

if [[ "$total" -gt 0 ]]; then
    exit 1
fi
exit 0
