# Lacunes Tracking Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `LACUNES.md` consistent now that all listed lacunes are complete, and record proof for completed `conductor/` plans.

**Architecture:** This is a documentation-only cleanup. Verify local proof first, then update only the tracking sections of `LACUNES.md`: replace stale recommended next steps with maintenance themes and add a conductor plan proof table.

**Tech Stack:** Markdown, `rg`, `sed`, Git.

---

## File Structure

- Modify: `LACUNES.md`
  - Replace the stale `## Prochaines etapes recommandees` list with `## Maintenance continue recommandee`.
  - Add `## Plans conductor traites` before `## Suivi des lots`.
  - Keep the 24 lacune statuses and historical lot entries intact.
- Read-only proof sources:
  - `src/assistant.rs`
  - `src/tools/web_search.rs`
  - `src/tui/events.rs`
  - `src/tui/widgets/tasks.rs`
  - `src/tui/layout.rs`
  - `src/tui/widgets/agent_panel.rs`
  - `src/repl.rs`
  - `src/tui/mod.rs`
  - `conductor/*.md`

## Task 1: Verify Existing Proofs

**Files:**
- Read: `conductor/*.md`
- Read: `src/assistant.rs`
- Read: `src/tools/web_search.rs`
- Read: `src/tui/events.rs`
- Read: `src/tui/widgets/tasks.rs`
- Read: `src/tui/layout.rs`
- Read: `src/tui/widgets/agent_panel.rs`
- Read: `src/repl.rs`
- Read: `src/tui/mod.rs`

- [ ] **Step 1: List conductor plans**

Run:

```bash
rg --files conductor
```

Expected output includes exactly these tracked plan notes:

```text
conductor/responsive-agents-grid.md
conductor/task-management-general.md
conductor/improve-ddg-parser.md
conductor/task-management-plan.md
conductor/phantom-assistant-fix.md
conductor/bare-tool-tags.md
```

- [ ] **Step 2: Verify bare tool tag parsing proof**

Run:

```bash
rg -n "parses_bare_tool_tags|parse_tool_calls|parse_single_call|parse_json_call|extract_tag" src/assistant.rs
```

Expected: matches for parser functions and tests named `parses_bare_tool_tags_with_raw_text` and `parses_bare_tool_tags_without_wrapper`.

- [ ] **Step 3: Verify DuckDuckGo Lite parser proof**

Run:

```bash
rg -n "search_without_key|parse_ddg_lite_html|DuckDuckGo Lite|result-link|result-snippet" src/tools/web_search.rs
```

Expected: matches for `search_without_key`, `parse_ddg_lite_html`, and structured result extraction.

- [ ] **Step 4: Verify phantom assistant label proof**

Run:

```bash
rg -n "agent: \"cortex\"|agent == \"cortex\"|strip_tool_calls_for_display|search_without_key" src/assistant.rs src/repl.rs src/tui/mod.rs
```

Expected: matches showing TUI/repl events use the `cortex` label, display stripping exists, and assistant web search can fall back to keyless search.

- [ ] **Step 5: Verify task-management proof**

Run:

```bash
rg -n "TASKS.md|TasksUpdated|parse_checklist_tasks|should_track_assistant_task|TasksWidget|tasks:" src/assistant.rs src/tui/events.rs src/tui/widgets/tasks.rs src/tui/layout.rs src/tui/mod.rs
```

Expected: matches for assistant task tracking, `TuiEvent::TasksUpdated`, the tasks widget, and layout/app task rendering.

- [ ] **Step 6: Verify responsive agent grid proof**

Run:

```bash
rg -n "min_col_width|max_cols|responsive|narrow|small|TestBackend" src/tui/widgets/agent_panel.rs
```

Expected: matches for responsive layout logic and headless render tests in `agent_panel.rs`.

## Task 2: Update `LACUNES.md`

**Files:**
- Modify: `LACUNES.md`

- [ ] **Step 1: Inspect the current tail section**

Run:

```bash
sed -n '250,320p' LACUNES.md
```

Expected: output shows `## Prochaines etapes recommandees` followed by stale numbered recommendations, then `## Suivi des lots`.

- [ ] **Step 2: Replace stale recommendations with maintenance themes and conductor proof table**

Use `apply_patch` to replace the section from `## Prochaines etapes recommandees` through the line before `## Suivi des lots` with this exact Markdown:

```markdown
## Maintenance continue recommandee

Les 24 lacunes identifiees dans ce document sont fermees pour le perimetre beta actuel. Les sujets ci-dessous restent des pratiques de maintenance continue, pas des lacunes ouvertes:

1. Etendre les evals avec des outputs reels de beta, un historique de campagnes et des tendances de qualite.
2. Maintenir le modele de menace et les tests adversariaux quand de nouveaux tools, providers, workflows custom, surfaces web/email ou mecanismes d'update sont ajoutes.
3. Revoir regulierement les recommandations providers/modeles, les limites connues et les estimations de cout.
4. Garder la checklist release et les smoke tests install/update a jour sur Linux, macOS et Windows.
5. Continuer a ameliorer la qualite des projets generes a partir des rapports utilisateurs et des echecs reels.
6. Garder `LACUNES.md` comme registre de fermeture des risques beta; placer les nouveaux chantiers produit dans `TASKS.md`, `conductor/` ou une roadmap dediee.

## Plans conductor traites

| Plan | Statut | Preuve |
|------|--------|--------|
| `conductor/bare-tool-tags.md` | Termine | `src/assistant.rs` parse les tags tools nus via `parse_tool_calls`/`parse_json_call` et couvre les cas `parses_bare_tool_tags_with_raw_text` et `parses_bare_tool_tags_without_wrapper`. |
| `conductor/improve-ddg-parser.md` | Termine | `src/tools/web_search.rs` expose `search_without_key()` et `parse_ddg_lite_html()` pour formatter des resultats DuckDuckGo Lite structures. |
| `conductor/phantom-assistant-fix.md` | Termine | Les evenements visibles utilisent le label `cortex` dans `src/assistant.rs`, `src/repl.rs` et `src/tui/mod.rs`; le meme lot couvre aussi le stripping tool XML et le fallback web search sans cle. |
| `conductor/responsive-agents-grid.md` | Termine | `src/tui/widgets/agent_panel.rs` contient la logique de grille responsive et des tests headless de rendu. |
| `conductor/task-management-general.md` | Termine | `src/assistant.rs` demande et maintient `TASKS.md` pour les taches complexes, parse les checklists, et publie `TuiEvent::TasksUpdated`. |
| `conductor/task-management-plan.md` | Termine | `src/tui/events.rs`, `src/tui/widgets/tasks.rs`, `src/tui/layout.rs` et `src/tui/mod.rs` definissent et rendent le panneau de taches. |
```

- [ ] **Step 3: Verify the edit is scoped**

Run:

```bash
git diff -- LACUNES.md
```

Expected: only the tail tracking section changes. Lacune status blocks and historical `Suivi des lots` entries remain intact.

## Task 3: Verify Tracking Consistency

**Files:**
- Read: `LACUNES.md`

- [ ] **Step 1: Search for stale open-status text**

Run:

```bash
rg -n "À faire|A faire|En cours|mode de run avec budget|Generer un `cortex.manifest.json`|templates GitHub Issues|cargo audit|cargo deny" LACUNES.md
```

Expected: no output.

- [ ] **Step 2: Check historical partial-treatment references remain only in lot history**

Run:

```bash
rg -n "partiellement traitees|partiellement traitées|partiellement traitée|partiellement traites|partiellement traités" LACUNES.md
```

Expected: output only from `## Suivi des lots` historical entries. Do not rewrite those entries; they are accurate historical snapshots.

- [ ] **Step 3: Confirm every conductor plan is represented**

Run:

```bash
rg -n "conductor/(bare-tool-tags|improve-ddg-parser|phantom-assistant-fix|responsive-agents-grid|task-management-general|task-management-plan)\\.md" LACUNES.md
```

Expected: six matches, one per conductor plan.

- [ ] **Step 4: Confirm the change remains docs-only**

Run:

```bash
git diff --stat
```

Expected: only `LACUNES.md` is modified.

## Task 4: Commit The Documentation Update

**Files:**
- Modify: `LACUNES.md`

- [ ] **Step 1: Review final diff**

Run:

```bash
git diff -- LACUNES.md
```

Expected: the diff replaces stale next steps with maintenance guidance and adds the conductor proof table.

- [ ] **Step 2: Commit**

Run:

```bash
git add LACUNES.md
git commit -m "docs: consolidate lacunes tracking"
```

Expected: one commit that touches only `LACUNES.md`. Do not add unrelated untracked files such as `.DS_Store`, `.claude/`, or `.idea/`.

## Self-Review Checklist

- Spec coverage: Tasks 1-4 cover proof verification, `LACUNES.md` cleanup, consistency checks, and commit.
- Placeholder scan: no placeholders or deferred implementation instructions are present.
- Scope check: the plan is documentation-only and does not touch runtime code.
