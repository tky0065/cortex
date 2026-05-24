# Local Release Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a local release smoke script that validates the current-platform release binary without touching the maintainer's global Cortex installation.

**Architecture:** Create a focused shell script under `scripts/` that builds `target/release/cortex`, copies it into an isolated temp directory, and runs deterministic non-destructive CLI checks with per-step logs. Document the pre-release workflow in `RELEASE.md`, then mark the maintenance lot complete in `LACUNES.md` after verification.

**Tech Stack:** POSIX-style shell script for macOS/Linux, Rust CLI built by Cargo, existing Markdown release/lacunes docs.

---

## File Structure

- Create `scripts/release_smoke.sh`: owns the local release smoke harness, temp workspace, command runner, logging, skip/pass/fail output, and cleanup behavior.
- Modify `RELEASE.md`: adds a pre-release local smoke section and keeps the existing post-release multi-platform smoke section as a separate published-binary check.
- Modify `LACUNES.md`: adds a dated tracking entry after the implementation passes, citing `scripts/release_smoke.sh` and `RELEASE.md`.

## Task 1: Add The Local Release Smoke Script

**Files:**
- Create: `scripts/release_smoke.sh`

- [ ] **Step 1: Create the script file**

Create `scripts/release_smoke.sh` with this exact content:

```sh
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
mkdir -p "$LOG_DIR" "$BIN_DIR"

cleanup() {
    status=$?
    if [ "$status" -eq 0 ] && [ "$KEEP_TEMP" -eq 0 ]; then
        rm -rf "$TMP_DIR"
    else
        echo "Temporary workspace: $TMP_DIR"
        echo "Logs: $LOG_DIR"
    fi
}
trap cleanup EXIT INT TERM

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

run_step "cargo build release" cargo build --release

SOURCE_BIN="$REPO_ROOT/target/release/cortex"
SMOKE_BIN="$BIN_DIR/cortex"
if [ ! -x "$SOURCE_BIN" ]; then
    echo "FAIL release binary missing or not executable: $SOURCE_BIN" >&2
    exit 1
fi

cp "$SOURCE_BIN" "$SMOKE_BIN"
chmod 755 "$SMOKE_BIN"

run_step "cortex version" "$SMOKE_BIN" --version
run_step "cortex help" "$SMOKE_BIN" --help
run_step "cortex start help" "$SMOKE_BIN" start --help
run_step "cortex run help" "$SMOKE_BIN" run --help
run_step "cortex resume help" "$SMOKE_BIN" resume --help
run_step "cortex update help" "$SMOKE_BIN" update --help
run_step "cortex skill help" "$SMOKE_BIN" skill --help

VALIDATE_DIR="$TMP_DIR/validate-project"
mkdir -p "$VALIDATE_DIR" "$TMP_DIR/home"
run_step "cortex validate empty project" sh -c 'cd "$1" && HOME="$2/home" "$3" validate' sh "$VALIDATE_DIR" "$TMP_DIR" "$SMOKE_BIN"

if [ "$RUN_UPDATE_CHECK" -eq 1 ]; then
    run_step "cortex update check" "$SMOKE_BIN" update --check
else
    printf 'SKIP cortex update check (network-dependent; pass --update-check to run)\n'
fi

printf 'PASS local release smoke completed\n'
```

- [ ] **Step 2: Make the script executable**

Run:

```bash
chmod +x scripts/release_smoke.sh
```

Expected: no output and exit code `0`.

- [ ] **Step 3: Run shell syntax validation**

Run:

```bash
sh -n scripts/release_smoke.sh
```

Expected: no output and exit code `0`.

- [ ] **Step 4: Commit the script**

Run:

```bash
git add scripts/release_smoke.sh
git commit -m "chore: add local release smoke script"
```

Expected: commit succeeds and only `scripts/release_smoke.sh` is included.

## Task 2: Verify And Tighten The Script Locally

**Files:**
- Modify: `scripts/release_smoke.sh`

- [ ] **Step 1: Run the smoke script**

Run:

```bash
scripts/release_smoke.sh --keep-temp
```

Expected: output contains these lines:

```text
PASS cargo build release
PASS cortex version
PASS cortex help
PASS cortex start help
PASS cortex run help
PASS cortex resume help
PASS cortex update help
PASS cortex skill help
PASS cortex validate empty project
SKIP cortex update check (network-dependent; pass --update-check to run)
PASS local release smoke completed
```

- [ ] **Step 2: Fix any command mismatch using actual CLI help**

If Step 1 fails because a subcommand name differs from the plan, inspect the current help:

```bash
target/release/cortex --help
```

Then update only the failing `run_step` command in `scripts/release_smoke.sh`. For example, if a subcommand help check needs an explicit `--help`, keep the pattern:

```sh
run_step "cortex update help" "$SMOKE_BIN" update --help
```

Expected: the script calls only subcommands that exist in `src/main.rs`.

- [ ] **Step 3: Re-run the smoke script after fixes**

Run:

```bash
scripts/release_smoke.sh
```

Expected: same pass/skip lines as Step 1, and no `Temporary workspace:` line on success because the script cleans up by default.

- [ ] **Step 4: Commit any script fix**

If Step 2 changed the script, run:

```bash
git add scripts/release_smoke.sh
git commit -m "fix: align release smoke with cli"
```

Expected: commit succeeds. If no fix was needed, skip this commit.

## Task 3: Document The Local Release Smoke Workflow

**Files:**
- Modify: `RELEASE.md`

- [ ] **Step 1: Add a local smoke section after the code quality checklist**

In `RELEASE.md`, insert this section after the `### 1. Code quality` checklist and before `### 2. Evals`:

````markdown
### 2. Local release smoke

- [ ] `scripts/release_smoke.sh` passes on the maintainer's current platform

The local release smoke builds `target/release/cortex`, copies the binary into an isolated temporary directory, and runs non-destructive CLI checks against that copy. It does not modify the maintainer's global Cortex installation and does not require provider credentials.

```bash
scripts/release_smoke.sh
```

Use `--keep-temp` to preserve logs after a successful run:

```bash
scripts/release_smoke.sh --keep-temp
```

The updater install path is not run by default because `cortex update` replaces the current executable. To run the network-only update availability check, use:

```bash
scripts/release_smoke.sh --update-check
```

If the script fails, inspect the log path printed in the failure output before tagging a release.
````

- [ ] **Step 2: Renumber the following release checklist sections**

Update the following headings in `RELEASE.md`:

```markdown
### 2. Evals
### 3. Documentation
### 4. Version bump
### 5. Tag
### 6. Post-release smoke tests
### 7. Checksums
### 8. Rollback
```

to:

```markdown
### 3. Evals
### 4. Documentation
### 5. Version bump
### 6. Tag
### 7. Post-release smoke tests
### 8. Checksums
### 9. Rollback
```

- [ ] **Step 3: Run a documentation sanity check**

Run:

```bash
rg -n "### [0-9]+\\." RELEASE.md
```

Expected output headings are sequential from `1` through `9`.

- [ ] **Step 4: Commit the release documentation**

Run:

```bash
git add RELEASE.md
git commit -m "docs: document local release smoke"
```

Expected: commit succeeds and only `RELEASE.md` is included.

## Task 4: Mark The Maintenance Lot Complete

**Files:**
- Modify: `LACUNES.md`

- [ ] **Step 1: Add a tracking entry**

At the end of the `## Suivi des lots` list in `LACUNES.md`, add:

```markdown
- 2026-05-24 — Lot release smoke local terminé: script `scripts/release_smoke.sh` ajouté pour construire le binaire release courant, l'exécuter depuis un préfixe temporaire isolé, vérifier les chemins CLI non destructifs, conserver des logs exploitables en cas d'échec, et documenter le workflow dans `RELEASE.md`. Maintenance continue couverte: smoke tests install/update locaux pour la plateforme courante du mainteneur.
```

- [ ] **Step 2: Run the local smoke script as proof**

Run:

```bash
scripts/release_smoke.sh
```

Expected: output ends with:

```text
PASS local release smoke completed
```

- [ ] **Step 3: Run the Rust test suite**

Run:

```bash
cargo test
```

Expected: all tests pass. If this fails for an unrelated pre-existing reason, capture the failing test names and do not mark the lot complete until the failure is understood.

- [ ] **Step 4: Commit the lacunes update**

Run:

```bash
git add LACUNES.md
git commit -m "docs: mark local release smoke complete"
```

Expected: commit succeeds and only `LACUNES.md` is included.

## Task 5: Final Verification

**Files:**
- Read-only verification of repository state

- [ ] **Step 1: Check worktree status**

Run:

```bash
git status --short
```

Expected: no tracked files are modified. Existing untracked local files such as `.DS_Store`, `.claude/`, or `.idea/` may remain if they were present before this work.

- [ ] **Step 2: Review recent commits**

Run:

```bash
git log --oneline -4
```

Expected: recent commits include:

```text
docs: mark local release smoke complete
docs: document local release smoke
chore: add local release smoke script
docs: design local release smoke
```

- [ ] **Step 3: Summarize verification evidence**

Final response should include:

```text
Implemented local release smoke coverage in scripts/release_smoke.sh, documented it in RELEASE.md, and marked the maintenance lot complete in LACUNES.md.
Verified with:
- scripts/release_smoke.sh
- cargo test
```

If any verification command could not be run or failed, state that directly with the failure summary.
