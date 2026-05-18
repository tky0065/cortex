# Beta Readiness Docs Design

## Context

`LACUNES.md` lists product and technical gaps for Cortex, but it is currently an audit document rather than a trackable backlog. `TASKS.md` is fully marked done, so the next useful step is to make beta-facing gaps visible, actionable, and partially closed with focused documentation.

This lot is documentation-only. It must not change Rust runtime behavior.

## Goals

- Turn `LACUNES.md` into a backlog that clearly shows open and completed items.
- Add concise beta documentation for positioning, supported workflows, provider guidance, and failure reporting.
- Mark only genuinely addressed documentation/process lacunes as completed.
- Link the new docs from `README.md` so users can find them.

## Non-Goals

- No changes to orchestration, providers, tools, TUI, auth, or workflow execution.
- No implementation of evals, cost tracking, run manifests, checkpoints, or security hardening.
- No claim that beta risks are solved at runtime when they are only documented.

## Scope

### `LACUNES.md`

Keep the existing sections and wording, but add visible tracking metadata for each lacune:

- `Statut: À faire`, `Statut: En cours`, or `Statut: Terminé`.
- `Preuve:` for completed items, pointing to the doc or template that closes the documentation/process gap.

For this lot, completion is limited to the docs/process beta lacunes that the new files directly cover.

### `docs/BETA.md`

Define the public beta stance:

- Recommended flagship workflow: `dev`.
- Other workflows are available but experimental unless proven by later evals.
- Clear limits of the beta promise.
- Short user path: install, connect provider, run a workflow, inspect outputs, report failures.
- Positioning language that avoids overpromising "full software company" outcomes.

### `docs/PROVIDERS.md`

Document provider expectations:

- Local vs remote provider trade-offs.
- Support levels for current provider families.
- Recommended model qualities by workflow class.
- Cost, latency, quota, privacy, and compatibility notes.
- Troubleshooting checklist for provider-caused failures.

### `.github/ISSUE_TEMPLATE/failed_run.md`

Add a focused issue template for failed Cortex runs:

- Workflow, command, provider/model, OS, Cortex version.
- Expected vs actual output.
- Safe logs and redaction guidance.
- Generated project quality symptoms.
- Reproduction steps.

### `README.md`

Add a small discoverability section linking to:

- Beta guide.
- Provider guide.
- Failed run reporting template.

## Acceptance Criteria

- `LACUNES.md` has clear statuses for all listed lacunes.
- Completed statuses are backed by concrete file references.
- `docs/BETA.md`, `docs/PROVIDERS.md`, and `.github/ISSUE_TEMPLATE/failed_run.md` exist and are internally consistent.
- `README.md` links to the new docs without duplicating them.
- The diff contains no Rust code changes.
- Link paths in the new and edited Markdown files are valid relative repository paths.

## Verification

- Review `git diff` for scope and consistency.
- Check that every `Terminé` item in `LACUNES.md` has a matching proof reference.
- Check that no Rust source files were modified.
