# Release Process

This document defines the steps required to publish a new Cortex release.

## Pre-release checklist

### 1. Code quality

- [ ] `cargo check --all-features` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test --all-features` passes
- [ ] `cargo audit` — no unresolved vulnerabilities
- [ ] `cargo deny check` — licenses and advisories clean

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

### 3. Evals

- [ ] Run `evals/run_campaign.sh` against the `dev` workflow with at least 3 scenarios
- [ ] All `required` checks pass
- [ ] No regressions compared to previous release (compare `evals/runs/` history)

### 4. Documentation

- [ ] `README.md` reflects new features or changed commands
- [ ] `RELEASE.md` (this file) is up to date
- [ ] `docs/PROMPT_CHANGELOG.md` updated if any prompt changed
- [ ] `CHANGELOG.md` entry written for the new version

### 5. Version bump

- [ ] Update `version` in `Cargo.toml`
- [ ] Run `cargo check` to propagate the version
- [ ] Commit: `chore: bump version to X.Y.Z`

### 6. Tag

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The `release.yml` GitHub Actions workflow builds binaries and creates the GitHub Release automatically.

### 7. Post-release smoke tests

Run manually on each supported platform after the binaries are published:

#### macOS (arm64 / x86_64)

```bash
curl -fsSL https://raw.githubusercontent.com/tky0065/cortex/main/install.sh | bash
cortex --version
cortex start "hello world CLI in Go" --auto --workflow dev
```

#### Linux (x86_64)

```bash
curl -fsSL https://raw.githubusercontent.com/tky0065/cortex/main/install.sh | bash
cortex --version
cortex start "hello world CLI in Go" --auto --workflow dev
```

#### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/tky0065/cortex/main/install.ps1 | iex
cortex --version
cortex start "hello world CLI in Go" --auto --workflow dev
```

Expected result: project directory created in `cortex-output/`, `cortex.manifest.json` present, no crash.

### 8. Checksums

The release workflow generates SHA-256 checksums for all binaries. Verify locally:

```bash
sha256sum -c cortex-vX.Y.Z-checksums.txt
```

### 9. Rollback

If a critical regression is found after release:

1. Delete the GitHub Release and tag.
2. Revert the offending commit.
3. Re-run the full checklist before re-tagging.

Do **not** reuse a version number once it has been published.

## Release cadence

There is no fixed cadence during beta. Release when the checklist passes and new features or fixes justify a release.
