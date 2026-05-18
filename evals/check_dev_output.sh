#!/usr/bin/env sh
set -eu

usage() {
  echo "Usage: evals/check_dev_output.sh <generated-project-dir> [scenario-file]" >&2
}

PROJECT_DIR="${1:-}"
SCENARIO_FILE="${2:-}"
SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd -P)"
SCENARIOS_DIR="$REPO_ROOT/evals/dev/scenarios"

if [ -z "$PROJECT_DIR" ]; then
  usage
  exit 2
fi

if [ ! -d "$PROJECT_DIR" ]; then
  echo "FAIL DEV-RUN-001 project directory does not exist: $PROJECT_DIR" >&2
  exit 1
fi

PROJECT_ROOT="$(CDPATH= cd "$PROJECT_DIR" && pwd -P)"

if [ -n "$SCENARIO_FILE" ] && [ ! -f "$SCENARIO_FILE" ]; then
  echo "FAIL DEV-RUN-002 scenario file does not exist: $SCENARIO_FILE" >&2
  exit 1
fi

if [ -n "$SCENARIO_FILE" ]; then
  scenario_dir="$(CDPATH= cd "$(dirname "$SCENARIO_FILE")" && pwd -P)"
  scenario_name="$(basename "$SCENARIO_FILE")"
  case "$scenario_name" in
    *.toml) ;;
    *)
      echo "FAIL DEV-RUN-005 scenario file must be a repository-owned .toml fixture: $SCENARIO_FILE" >&2
      exit 1
      ;;
  esac
  if [ "$scenario_dir" != "$SCENARIOS_DIR" ] || [ -L "$SCENARIO_FILE" ]; then
    echo "FAIL DEV-RUN-005 scenario file must be under evals/dev/scenarios/: $SCENARIO_FILE" >&2
    exit 1
  fi
fi

if [ -n "$SCENARIO_FILE" ] && ! grep -Eq '^[[:space:]]*required_files[[:space:]]*=[[:space:]]*\[' "$SCENARIO_FILE"; then
  echo "FAIL DEV-RUN-004 scenario file is missing required_files array: $SCENARIO_FILE" >&2
  exit 1
fi

failures=0
warnings=0
scenario_parse_failed=0

pass() {
  echo "PASS $1 $2"
}

fail() {
  echo "FAIL $1 $2"
  failures=$((failures + 1))
}

warn() {
  echo "WARN $1 $2"
  warnings=$((warnings + 1))
}

require_file() {
  file="$1"
  check_id="$2"
  if ! validate_required_file_entry "$file"; then
    fail "$check_id" "invalid required file path: $file"
    return
  fi

  path="$PROJECT_ROOT/$file"
  if [ -s "$path" ]; then
    pass "$check_id" "$file exists"
  else
    fail "$check_id" "$file is missing or empty"
  fi
}

validate_required_file_entry() {
  file="$1"
  if [ -z "$file" ]; then
    return 1
  fi
  case "$file" in
    /* | ../* | */../* | */.. | ..)
      return 1
      ;;
  esac
  return 0
}

extract_toml_array() {
  key="$1"
  file="$2"
  awk -v key="$key" '
    $0 ~ "^[[:space:]]*" key "[[:space:]]*=[[:space:]]*\\[" { active = 1; next }
    active {
      line = $0
      gsub(/^[[:space:]]+/, "", line)
      gsub(/[[:space:]]+$/, "", line)
      if (line == "]") {
        active = 0
        next
      }
      if (line == "") {
        next
      }
      if (line !~ /^"[^"]*",?$/) {
        print "__DEV_RUN_007__ " key " malformed TOML array item on line " NR
        next
      }
      sub(/^"/, "", line)
      sub(/",?$/, "", line)
      print line
    }
  ' "$file"
}

contains_parse_error() {
  case "$1" in
    *"__DEV_RUN_007__"*) return 0 ;;
    *) return 1 ;;
  esac
}

extract_required_files() {
  if [ -n "$SCENARIO_FILE" ]; then
    extract_toml_array "required_files" "$SCENARIO_FILE"
  fi
}

extract_commands() {
  if [ -n "$SCENARIO_FILE" ]; then
    extract_toml_array "commands" "$SCENARIO_FILE"
  fi
}

extract_required_command_binaries() {
  if [ -n "$SCENARIO_FILE" ]; then
    extract_toml_array "required_command_binaries" "$SCENARIO_FILE"
  fi
}

grep_scan() {
  regex="$1"
  find "$PROJECT_DIR" \
    -type d \( -name .git -o -name target -o -name node_modules -o -name .venv -o -name __pycache__ \) -prune \
    -o -type f -exec grep -InE "$regex" {} + 2>/dev/null || true
}

check_blocking_markers() {
  marker_regex='T[[:space:]]*O[[:space:]]*D[[:space:]]*O: implement|T[[:space:]]*B[[:space:]]*D|place[ -]?holder|lorem ipsum|unimplemented!|panic\("not implemented"\)'
  matches="$(grep_scan "$marker_regex")"
  if [ -n "$matches" ]; then
    echo "$matches" | sed 's/^/  /'
    fail "DEV-MAINT-001" "blocking implementation marker found"
  else
    pass "DEV-MAINT-001" "no blocking implementation markers found"
  fi
}

check_secret_patterns() {
  matches="$(grep_scan 'PRIVATE KEY|api[_-]?key[[:space:]]*=|token[[:space:]]*=|password[[:space:]]*=|secret[[:space:]]*=')"
  if [ -n "$matches" ]; then
    echo "$matches" | sed -E 's/(:).*/:\[redacted\]/' | sed 's/^/  /'
    fail "DEV-SEC-001" "possible hardcoded secret found"
  else
    pass "DEV-SEC-001" "no obvious hardcoded secrets found"
  fi
}

check_local_paths() {
  matches="$(grep_scan '/Users/|/home/|C:\\\\Users\\\\')"
  if [ -n "$matches" ]; then
    echo "$matches" | sed 's/^/  /'
    fail "DEV-SEC-003" "local machine path found"
  else
    pass "DEV-SEC-003" "no local machine paths found"
  fi
}

run_scenario_commands() {
  if [ -z "$SCENARIO_FILE" ]; then
    warn "DEV-BUILD-001" "no scenario file provided; stack commands skipped"
    return
  fi
  if [ "$scenario_parse_failed" -ne 0 ]; then
    return
  fi

  commands="$(extract_commands)"
  binaries="$(extract_required_command_binaries)"
  if contains_parse_error "$commands"; then
    fail "DEV-RUN-007" "$(printf '%s\n' "$commands" | grep '__DEV_RUN_007__' | sed 's/__DEV_RUN_007__ //')"
    return
  fi
  if contains_parse_error "$binaries"; then
    fail "DEV-RUN-007" "$(printf '%s\n' "$binaries" | grep '__DEV_RUN_007__' | sed 's/__DEV_RUN_007__ //')"
    return
  fi
  if [ -n "$commands" ] && [ -z "$binaries" ]; then
    fail "DEV-RUN-006" "scenario commands require required_command_binaries"
    return
  fi

  missing_binary=0
  while IFS= read -r binary; do
    if [ -z "$binary" ]; then
      continue
    fi
    if command -v "$binary" >/dev/null 2>&1; then
      pass "DEV-RUN-003" "required command binary available: $binary"
    else
      warn "DEV-RUN-003" "required command binary unavailable; commands skipped: $binary"
      missing_binary=1
    fi
  done <<EOF_BINARIES
$binaries
EOF_BINARIES

  if [ "$missing_binary" -ne 0 ]; then
    return
  fi

  while IFS= read -r command_line; do
    if [ -z "$command_line" ]; then
      continue
    fi
    if ! validate_scenario_command "$command_line" "$binaries"; then
      fail "DEV-RUN-006" "unsafe or undeclared scenario command: $command_line"
      continue
    fi
    if ! is_allowed_scenario_command "$command_line"; then
      fail "DEV-RUN-009" "scenario command is not allowed: $command_line"
      continue
    fi
    echo "RUN $command_line"
    if run_validated_command "$command_line"; then
      pass "DEV-BUILD-001" "command passed: $command_line"
    else
      fail "DEV-BUILD-001" "command failed: $command_line"
    fi
  done <<EOF_COMMANDS
$commands
EOF_COMMANDS
}

validate_scenario_command() {
  command_line="$1"
  binaries="$2"
  case "$command_line" in
    *';'* | *'|'* | *'&'* | *'<'* | *'>'* | *'`'* | *'$'* | *'('* | *')'* | *'{'* | *'}'* | *'['* | *']'* | *'*'* | *'?'* | *'!'* | *\\*)
      return 1
      ;;
  esac
  tab="$(printf '\t')"
  newline='
'
  case "$command_line" in
    *"$tab"* | *"$newline"*)
      return 1
      ;;
  esac
  set -- $command_line
  if [ "$#" -eq 0 ]; then
    return 1
  fi
  first_word="$1"
  found=0
  while IFS= read -r binary; do
    if [ "$first_word" = "$binary" ]; then
      found=1
    fi
  done <<EOF_BINARIES
$binaries
EOF_BINARIES
  [ "$found" -eq 1 ]
}

is_allowed_scenario_command() {
  command_line="$1"
  case "$command_line" in
    "cargo test" | "python3 -m pytest" | "npm test")
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

run_validated_command() {
  command_line="$1"
  set -- $command_line
  (cd "$PROJECT_ROOT" && "$@")
}

require_file "specs.md" "DEV-ART-001"
require_file "architecture.md" "DEV-ART-002"
require_file "README.md" "DEV-DOC-001"

if [ -n "$SCENARIO_FILE" ]; then
  required_files_tmp="$(mktemp "${TMPDIR:-/tmp}/cortex-required-files.XXXXXX")"
  extract_required_files > "$required_files_tmp"
  if grep -q '__DEV_RUN_007__' "$required_files_tmp"; then
    fail "DEV-RUN-007" "$(grep '__DEV_RUN_007__' "$required_files_tmp" | sed 's/__DEV_RUN_007__ //')"
    scenario_parse_failed=1
  fi
  while IFS= read -r file; do
    case "$file" in
      *__DEV_RUN_007__*)
        continue
        ;;
    esac
    require_file "$file" "DEV-STRUCT-001"
  done < "$required_files_tmp"
else
  warn "DEV-STRUCT-001" "no scenario file provided; scenario required files skipped"
fi

check_blocking_markers
check_secret_patterns
check_local_paths
run_scenario_commands

echo "SUMMARY failures=$failures warnings=$warnings"

if [ "$failures" -gt 0 ]; then
  exit 1
fi
