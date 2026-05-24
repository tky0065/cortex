#!/usr/bin/env sh
set -eu

KEEP_TEMP=0
RUN_UPDATE_CHECK=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --keep-temp)
            KEEP_TEMP=1
            ;;
        --update-check)
            RUN_UPDATE_CHECK=1
            ;;
        -h|--help)
            cat <<'USAGE'
Usage: scripts/release_smoke.sh [--keep-temp] [--update-check]

Builds the current Cortex release binary, copies it into an isolated
temporary workspace, and runs safe local smoke checks against that copy.

Options:
  --keep-temp      Keep the temporary workspace after a successful run.
  --update-check   Also run `cortex update --check` against GitHub Releases.
                  This is network-dependent and never installs an update.
USAGE
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            echo "Try: scripts/release_smoke.sh --help" >&2
            exit 2
            ;;
    esac
    shift
done

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$REPO_ROOT"

case "$(uname -s)" in
    Darwin|Linux)
        ;;
    *)
        echo "SKIP unsupported OS for local release smoke: $(uname -s)"
        echo "This script currently supports macOS and Linux only."
        exit 0
        ;;
esac

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cortex-release-smoke.XXXXXX")
LOG_DIR="$TMP_DIR/logs"
BIN_DIR="$TMP_DIR/bin"
SMOKE_HOME="$TMP_DIR/home"
mkdir -p "$LOG_DIR" "$BIN_DIR" "$SMOKE_HOME"

cleanup() {
    status=$?
    if [ "$status" -eq 0 ] && [ "$KEEP_TEMP" -eq 0 ]; then
        rm -rf "$TMP_DIR"
    else
        echo "Temporary workspace: $TMP_DIR"
        echo "Logs: $LOG_DIR"
    fi
}

on_signal() {
    trap - EXIT INT TERM
    cleanup
    exit "$1"
}

trap cleanup EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

step_slug() {
    printf '%s' "$1" | tr '[:upper:] ' '[:lower:]-' | tr -cd '[:alnum:]-_'
}

run_step() {
    name=$1
    shift
    slug=$(step_slug "$name")
    log="$LOG_DIR/$slug.log"
    printf 'RUN  %s\n' "$name"
    if "$@" >"$log" 2>&1; then
        printf 'PASS %s\n' "$name"
    else
        printf 'FAIL %s\n' "$name" >&2
        printf 'Log: %s\n' "$log" >&2
        exit 1
    fi
}

run_step "cargo build release" cargo build --release --locked

SOURCE_BIN="$REPO_ROOT/target/release/cortex"
SMOKE_BIN="$BIN_DIR/cortex"
if [ ! -x "$SOURCE_BIN" ]; then
    echo "FAIL release binary missing or not executable: $SOURCE_BIN" >&2
    exit 1
fi

cp "$SOURCE_BIN" "$SMOKE_BIN"
chmod 755 "$SMOKE_BIN"

run_cortex() {
    HOME="$SMOKE_HOME" "$SMOKE_BIN" "$@"
}

run_step "cortex version" run_cortex --version
run_step "cortex help" run_cortex --help
run_step "cortex start help" run_cortex start --help
run_step "cortex run help" run_cortex run --help
run_step "cortex resume help" run_cortex resume --help
run_step "cortex update help" run_cortex update --help
run_step "cortex skill help" run_cortex skill --help

VALIDATE_DIR="$TMP_DIR/validate-project"
mkdir -p "$VALIDATE_DIR"
run_step "cortex validate empty project" sh -c 'cd "$1" && HOME="$2" "$3" validate' sh "$VALIDATE_DIR" "$SMOKE_HOME" "$SMOKE_BIN"

if [ "$RUN_UPDATE_CHECK" -eq 1 ]; then
    run_step "cortex update check" run_cortex update --check
else
    printf 'SKIP cortex update check (network-dependent; pass --update-check to run)\n'
fi

printf 'PASS local release smoke completed\n'
