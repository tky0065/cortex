# Local Release Smoke Design

## Context

`LACUNES.md` marks the original beta-readiness gaps as complete. Its maintenance section still calls out release QA as an ongoing practice: keep install and update smoke tests current across release paths. `RELEASE.md` already defines a release checklist, but the local pre-release verification can be made more repeatable with a single script that exercises the current platform without changing the maintainer's global installation.

The next lot should add a local release smoke test that a maintainer can run before tagging or publishing a release. It should validate the release binary and safe CLI paths on the maintainer's current operating system only.

## Goals

- Add a local release smoke script for the current maintainer platform.
- Build the release binary from the working tree.
- Install or copy that binary into an isolated temporary prefix.
- Verify non-destructive CLI paths such as version/help and safe diagnostics.
- Exercise the update path only when a safe dry-run or equivalent behavior exists.
- Produce clear pass/fail output and a non-zero exit code on failure.
- Document the script in `RELEASE.md`.
- Mark the maintenance lot complete in `LACUNES.md` with proof references.

## Non-Goals

- Do not add a GitHub Actions matrix for this lot.
- Do not test Linux, macOS, and Windows from one machine.
- Do not modify the user's global `cortex` installation.
- Do not publish, tag, upload assets, or call external release services.
- Do not run provider-backed workflows that require API keys or model access.
- Do not replace the existing eval harness.

## Recommended Approach

Add `scripts/release_smoke.sh`.

The script should create a temporary workspace, build `target/release/cortex`, copy the binary into the temporary workspace, and run a small set of safe commands through that copied binary. The default checks should be deterministic and offline-friendly:

- `cortex --version`
- `cortex --help`
- safe subcommand help screens that exist in the current CLI
- a validation or diagnostic path that does not require network access, secrets, or writing to user directories

If the updater already exposes a dry-run or verification-only mode, the script should include it. If not, the script should report that update smoke coverage is skipped with a clear reason instead of inventing a fake update test.

This approach is preferred because it gives the release maintainer a repeatable local gate while staying small enough to maintain. It also avoids duplicating the heavier eval harness, which is better suited for generated-project quality.

## Alternatives Considered

### Documentation-Only Checklist

Adding commands to `RELEASE.md` would be simple, but it would not provide consistent pass/fail behavior or preserve logs from a failed smoke run.

### Full Multi-Platform CI Smoke

A CI matrix would improve platform coverage, but the user chose local-only coverage for this lot. CI can be added later once the local script has stabilized.

### Heavy End-To-End Workflow Smoke

Running a generated project through Cortex would test more behavior, but it would be slower, provider-dependent, and partly duplicate `evals/`. The release smoke should focus on installation and safe CLI behavior.

## Script Behavior

The script should:

1. Resolve the repository root from its own location.
2. Create a temporary directory under the system temp location.
3. Build the release binary with `cargo build --release`.
4. Copy the built binary into the temporary directory.
5. Run each smoke command using the copied binary.
6. Write command output to per-step log files.
7. Print concise step status lines.
8. Preserve the temp directory path on failure.
9. Clean up the temp directory on success unless a keep flag is provided.

The script should use shell features that work on common macOS and Linux environments. Windows is out of scope for this local-current-platform lot.

## Documentation Changes

Update `RELEASE.md` with a short section explaining:

- when to run the local release smoke test;
- the command to run;
- what the script covers;
- what it intentionally does not cover;
- how to inspect logs after a failure.

Update `LACUNES.md` maintenance tracking with a new dated lot entry once implementation is complete. The proof should cite `scripts/release_smoke.sh` and `RELEASE.md`.

## Error Handling

The script should fail fast when a required command fails. Each failure message should include:

- the failed step name;
- the log file path;
- the temporary workspace path.

Expected skips, such as unavailable safe updater coverage, should be shown as `SKIP` rather than `PASS`.

## Verification

Before implementation, inspect the CLI help and updater command surface so the script only calls commands that actually exist.

After implementation:

- run `scripts/release_smoke.sh` on the current machine;
- run `cargo test` if any Rust code changes are required;
- inspect `git diff` to confirm the change is limited to the smoke script and documentation unless CLI support is needed.

## Success Criteria

- A maintainer can run one local command before release.
- The command validates the release binary from the current working tree.
- The command does not alter the maintainer's global installation or require network/provider credentials.
- Failures are actionable through retained logs.
- `RELEASE.md` documents the workflow.
- `LACUNES.md` records the lot as complete after implementation.
