#!/usr/bin/env sh
# evals/run_campaign.sh — Run all dev eval scenarios and produce a scored report.
#
# Usage:
#   evals/run_campaign.sh <projects-base-dir>
#
# <projects-base-dir> must contain one subdirectory per scenario named after the
# scenario file (without the .toml extension). For example:
#
#   projects-base-dir/
#     rust_json_cli/          # output for evals/dev/scenarios/rust_json_cli.toml
#     http_api_minimal/
#     python_file_tool/
#
# The script scores each project against its scenario, writes a JSON run record
# to evals/runs/<timestamp>.json, and prints a human-readable summary.
#
# Exit code:
#   0  All required checks passed for every scenario.
#   1  At least one required check failed.

set -eu

SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd -P)"
SCENARIOS_DIR="$SCRIPT_DIR/dev/scenarios"
CHECK_SCRIPT="$SCRIPT_DIR/check_dev_output.sh"
RUNS_DIR="$SCRIPT_DIR/runs"
TIMESTAMP="$(date -u '+%Y%m%dT%H%M%SZ')"
REPORT_FILE="$RUNS_DIR/$TIMESTAMP.json"

PROJECTS_BASE="${1:-}"
if [ -z "$PROJECTS_BASE" ]; then
  echo "Usage: evals/run_campaign.sh <projects-base-dir>" >&2
  exit 2
fi
if [ ! -d "$PROJECTS_BASE" ]; then
  echo "ERROR: projects base directory does not exist: $PROJECTS_BASE" >&2
  exit 2
fi

mkdir -p "$RUNS_DIR"

# ─── Counters ─────────────────────────────────────────────────────────────────
total=0
passed=0
failed=0
skipped=0

# ─── JSON accumulator ─────────────────────────────────────────────────────────
json_results='[]'

append_json() {
  scenario="$1"
  result="$2"
  detail="$3"
  json_results="$(printf '%s' "$json_results" | sed 's/\]$//')"
  if [ "$json_results" = '[' ] || [ "$json_results" = "[]" ]; then
    json_results="[{\"scenario\":\"$scenario\",\"result\":\"$result\",\"detail\":\"$detail\"}]"
  else
    json_results="${json_results},{\"scenario\":\"$scenario\",\"result\":\"$result\",\"detail\":\"$detail\"}]"
  fi
}

# ─── Run each scenario ────────────────────────────────────────────────────────
for scenario_file in "$SCENARIOS_DIR"/*.toml; do
  scenario_name="$(basename "$scenario_file" .toml)"
  project_dir="$PROJECTS_BASE/$scenario_name"
  total=$((total + 1))

  if [ ! -d "$project_dir" ]; then
    echo "SKIP  $scenario_name  (no project dir: $project_dir)"
    skipped=$((skipped + 1))
    append_json "$scenario_name" "skip" "no project directory"
    continue
  fi

  # Capture output; treat non-zero exit as failure.
  set +e
  check_output="$("$CHECK_SCRIPT" "$project_dir" "$scenario_file" 2>&1)"
  check_exit=$?
  set -e

  if [ $check_exit -eq 0 ]; then
    echo "PASS  $scenario_name"
    passed=$((passed + 1))
    append_json "$scenario_name" "pass" ""
  else
    # Extract first FAIL line as short detail
    first_fail="$(printf '%s\n' "$check_output" | grep '^FAIL' | head -1)"
    echo "FAIL  $scenario_name  — $first_fail"
    failed=$((failed + 1))
    append_json "$scenario_name" "fail" "$first_fail"
  fi
done

# ─── Write JSON report ────────────────────────────────────────────────────────
cat > "$REPORT_FILE" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "projects_base": "$PROJECTS_BASE",
  "summary": {
    "total": $total,
    "passed": $passed,
    "failed": $failed,
    "skipped": $skipped
  },
  "results": $json_results
}
EOF

# ─── Print summary ────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════"
echo "  Campaign: $TIMESTAMP"
echo "  Total:    $total"
echo "  Passed:   $passed"
echo "  Failed:   $failed"
echo "  Skipped:  $skipped"
echo "  Report:   $REPORT_FILE"
echo "═══════════════════════════════════════"

if [ $failed -gt 0 ]; then
  exit 1
fi
exit 0
