# Lacunes Tracking Consolidation Design

## Context

`LACUNES.md` now marks all 24 listed project gaps as complete. The follow-up section still contains older recommended next steps, several of which are already closed by later lots. The `conductor/` directory also contains implementation notes for work that appears to have landed, including bare tool tag parsing, DuckDuckGo Lite parsing, task tracking, responsive agent panels, and the phantom assistant label fix.

This creates a tracking problem: the project has strong proof artifacts, but the top-level gap document still reads partly like an active backlog. The next lot should consolidate the tracking layer so a maintainer can tell what is done, what proof exists, and what remains ongoing maintenance.

## Goals

- Make `LACUNES.md` internally consistent after all listed lacunes have been closed.
- Replace stale "recommended next steps" with a maintenance-focused section.
- Add explicit proof references for completed `conductor/` notes.
- Keep the update documentation-only unless verification exposes a concrete missing proof.
- Preserve historical lot tracking instead of rewriting past entries.
- Mark completed work clearly while avoiding claims that future quality, security, or eval work is finished forever.

## Non-Goals

- Do not change runtime behavior.
- Do not refactor the TUI, assistant, tools, providers, or workflows.
- Do not add new product features.
- Do not reopen completed lacunes unless a cited proof is missing.
- Do not delete historical plans or specs.

## Recommended Approach

Use a conservative documentation cleanup.

First, verify each `conductor/*.md` note against local code, tests, or docs. Then update `LACUNES.md` in three places:

1. Keep all 24 lacunes marked `Terminé`.
2. Replace stale "Prochaines etapes recommandees" entries with a "Maintenance continue" section.
3. Add a "Plans conductor traites" section that maps each conductor plan to its current proof.

This is preferable to adding new lacunes because the existing file is explicitly a gap closure tracker. New roadmap work should live in a roadmap or task plan, not be mixed into a document whose main purpose is to record closed beta-readiness gaps.

## Alternatives Considered

### Leave `LACUNES.md` As Is

This avoids churn, but leaves contradictions: completed items still appear as recommended next steps. That weakens the document as a beta-readiness artifact.

### Turn `LACUNES.md` Into A Full Roadmap

This would capture more future work, but it would blur the difference between closed gaps and ongoing product improvement. A focused maintenance section is clearer.

### Move Completed Conductor Notes Elsewhere

Archiving or moving `conductor/` notes would reduce clutter, but it is unnecessary for this lot and risks hiding useful implementation history.

## Documentation Changes

### `LACUNES.md`

Update the "Prochaines etapes recommandees" section to say that the original lacunes are closed and future work is maintenance. Suggested maintenance themes:

- extend evals with real beta outputs and historical trends;
- keep the threat model and adversarial tests current as new tools/providers land;
- review provider pricing and model recommendations over time;
- keep release QA checks current across install/update paths;
- continue improving generated-project quality based on user reports.

Add a "Plans conductor traites" section with rows for:

- `conductor/bare-tool-tags.md`: proof in `src/assistant.rs` parser tests for bare tool tags.
- `conductor/improve-ddg-parser.md`: proof in `src/tools/web_search.rs` structured DuckDuckGo Lite parser.
- `conductor/phantom-assistant-fix.md`: proof in `src/assistant.rs`, `src/repl.rs`, and `src/tui/mod.rs` using `cortex` labels plus parser/web-search updates.
- `conductor/responsive-agents-grid.md`: proof in `src/tui/widgets/agent_panel.rs` responsive layout tests or implementation.
- `conductor/task-management-general.md`: proof in `src/assistant.rs` `TASKS.md` tracking and `TuiEvent::TasksUpdated`.
- `conductor/task-management-plan.md`: proof in `src/tui/events.rs`, `src/tui/widgets/tasks.rs`, `src/tui/layout.rs`, and TUI task rendering.

The wording should be factual and cite files, not broad claims.

## Verification

Run local searches before editing:

- search for conductor feature names in `src/`;
- search for stale "prochaines etapes" entries that duplicate completed lacunes;
- check that cited files exist.

After editing:

- run `rg -n "À faire|A faire|En cours|partiellement traitées|mode de run avec budget|cortex.manifest|templates GitHub|cargo audit" LACUNES.md` to catch stale status text;
- run `git diff -- LACUNES.md` and confirm the diff is documentation-only.

No Rust test is required if only `LACUNES.md` changes. If the update cites a specific test name, verify that test exists by search.

## Success Criteria

- `LACUNES.md` no longer lists already completed items as recommended next steps.
- Every `conductor/*.md` plan has an explicit status/proof row.
- The document distinguishes closed beta gaps from ongoing maintenance.
- The change is limited to documentation.
