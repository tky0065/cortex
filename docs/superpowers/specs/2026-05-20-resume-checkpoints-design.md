# Resume Checkpoints Design

## Context

`LACUNES.md` lists lacune 9 as open: Cortex can resume an interrupted run, but the current behavior mainly relaunches the `dev` workflow in the same project directory with a generic "resume and complete" prompt. It does not know which phase was last completed, which files belong to that run, whether those files were changed after interruption, or which action should happen next.

The run observability lot added `cortex.run.json`, which explains what happened during a run. That report is useful for diagnostics, but it is not a control-plane artifact for safe resume. Cortex needs a dedicated checkpoint file that represents recoverable workflow state.

## Goals

- Add an explicit `cortex.checkpoint.json` artifact for structured resume.
- Support robust phase-level resume for the built-in `dev` workflow first.
- Track stable phase boundaries, completed phases, next action, and generated files.
- Hash tracked files so resume can detect missing or modified files before writing.
- Refuse ambiguous resume attempts instead of silently rerunning the whole workflow.
- Preserve user changes by default; no automatic overwrite or merge when conflicts are detected.
- Update `README.md` and `LACUNES.md` after implementation.

## Non-Goals

- Do not implement automatic conflict resolution or three-way merges.
- Do not add full checkpoint support for marketing, prospecting, code-review, or custom workflows in this lot.
- Do not replace `cortex.run.json`; it remains the diagnostic report.
- Do not replace `cortex.manifest.json`; it remains the generated project identity artifact for successful runs.
- Do not redesign the TUI resume picker beyond using the safer resume path.
- Do not guarantee resume from the middle of an in-flight parallel worker group. Checkpoints are written only at stable phase boundaries.

## Recommended Approach

Create a focused checkpoint module and wire it into the orchestrator and `DevWorkflow`. The orchestrator owns loading a checkpoint for `cortex resume <dir>` and passing resume context into `RunOptions`. The workflow owns writing checkpoints at semantic phase boundaries and deciding which phases can be skipped.

This keeps resume state explicit and testable without forcing every workflow to adopt the same implementation immediately. The checkpoint schema should be generic enough for future workflows, but this lot should only mark `dev` resume as supported.

## Alternatives Considered

### Infer Resume From Existing Files

Cortex could inspect `specs.md`, `architecture.md`, source files, Dockerfiles, and reports to guess where to resume. This is fast to add, but fragile. A present file does not prove it belongs to the interrupted run, matches the original prompt, or was not edited by the user.

### Generic Workflow State API For All Workflows

Cortex could add a trait-level checkpoint API for every workflow and custom workflow. This is cleaner long term, but too broad for this lot because each workflow has different phase semantics and output directories.

### Use `cortex.run.json` As Resume State

The run report already contains timeline and file records, but it is optimized for diagnostics and sharing. Reusing it for control flow would couple report retention, redaction, and resume semantics too tightly.

## Architecture

### `src/checkpoint.rs`

Add a new module that owns checkpoint data structures, JSON persistence, file hashing, validation, and conflict detection.

Core public API:

- `Checkpoint::new(run_id, workflow, prompt, config)`
- `Checkpoint::load(project_dir)`
- `Checkpoint::write_to(project_dir, config)`
- `Checkpoint::record_phase_complete(phase, next_action)`
- `Checkpoint::record_file(agent, phase, path, operation, project_dir)`
- `Checkpoint::validate_files(project_dir) -> Vec<CheckpointConflict>`
- `Checkpoint::is_resume_supported_for(workflow)`

The module should redact known secrets before writing prompt-like fields, using `SecretRedactor::from_config_and_env()`, matching the run report and manifest behavior.

### `RunOptions`

Extend `RunOptions` with a small resume context:

```rust
pub struct ResumeContext {
    pub checkpoint: Checkpoint,
    pub conflicts: Vec<CheckpointConflict>,
}
```

`RunOptions` should carry `resume: Option<ResumeContext>`. Built-in workflows that do not support resume can ignore it for now, but the orchestrator should only pass it when a checkpoint was explicitly loaded.

### Orchestrator Integration

Add a resume-aware path to `run_with_project_dir()` or a dedicated wrapper used by CLI and REPL resume commands.

On normal runs:

- Create a new checkpoint for workflows that support it.
- Pass it to the workflow as mutable/resumable state through `RunOptions`.
- Do not require existing checkpoint files.

On resume runs:

- Require `cortex.checkpoint.json` in the target directory.
- Load and parse the checkpoint.
- Verify the checkpoint workflow matches the workflow being resumed.
- Validate tracked file hashes.
- If conflicts exist, abort before running agents and emit a clear TUI/CLI error.
- If valid, pass the checkpoint into `RunOptions` and run in auto mode for this lot.

The current CLI and REPL resume commands should stop hardcoding a generic prompt as the primary source of truth. The original prompt should come from the checkpoint. The command can still display a resume message.

### Dev Workflow Integration

`DevWorkflow` should write checkpoints only at stable boundaries:

- `started`
- `brief-ready`
- `specs-ready`
- `architecture-ready`
- `development-done`
- `qa-approved` or `qa-max-iterations`
- `devops-done`
- `done`

On resume, `DevWorkflow` should skip phases that are already completed and whose required files are valid:

- If `brief-ready` is complete, reuse the stored brief from the checkpoint.
- If `specs-ready` is complete, read `specs.md` from disk and skip PM.
- If `architecture-ready` is complete, read `architecture.md` from disk and skip Tech Lead.
- If `development-done` is complete, skip developer generation and proceed to QA or DevOps depending on `next_action`.
- If `qa-approved` is complete, skip directly to DevOps.

The checkpoint should not attempt to persist large raw agent outputs beyond the values needed to resume. For `dev`, storing the CEO brief is acceptable because downstream PM depends on it before `specs.md` exists. Once `specs.md` and `architecture.md` exist, disk files are the source of truth.

## Data Model

`cortex.checkpoint.json` should use stable, obvious field names:

```json
{
  "schema_version": 1,
  "run_id": "uuid",
  "cortex_version": "0.1.0",
  "workflow": "dev",
  "prompt": "redacted original prompt",
  "provider": "ollama",
  "status": "running",
  "current_phase": "architecture-ready",
  "completed_phases": ["started", "brief-ready", "specs-ready", "architecture-ready"],
  "next_action": "run_developer",
  "dev": {
    "brief": "redacted brief text",
    "specs_path": "specs.md",
    "architecture_path": "architecture.md",
    "expected_files": ["src/main.rs"],
    "qa_iteration": 0
  },
  "files": [
    {
      "path": "specs.md",
      "agent": "pm",
      "phase": "specs-ready",
      "operation": "created",
      "bytes": 1200,
      "sha256": "hex",
      "updated_at_unix_ms": 1779235200000
    }
  ],
  "updated_at_unix_ms": 1779235200000
}
```

Allowed checkpoint statuses:

- `running`
- `interrupted`
- `failed`
- `completed`

Allowed conflict types:

- `checkpoint_missing`
- `unsupported_workflow`
- `workflow_mismatch`
- `invalid_checkpoint`
- `file_missing`
- `file_modified`
- `phase_inconsistent`

## Conflict Handling

Resume should be conservative:

- Missing checkpoint: abort with "structured resume requires cortex.checkpoint.json".
- Unsupported workflow: abort and say structured resume currently supports `dev`.
- Invalid JSON or schema: abort with the parse/schema error.
- Workflow mismatch: abort and show checkpoint workflow and requested workflow.
- Missing tracked file: abort and list the missing paths.
- Modified tracked file: abort and list paths with expected and current hashes.
- Inconsistent phase: abort and explain the missing prerequisite.

No conflict path should overwrite files. A future lot can add explicit user choices such as "accept local changes" or "rerun from phase".

## Documentation

Update `README.md`:

- Explain that `cortex resume <dir>` requires `cortex.checkpoint.json`.
- Explain the difference between `cortex.checkpoint.json`, `cortex.run.json`, and `cortex.manifest.json`.
- Document that resume detects modified files and stops before overwriting.

Update `LACUNES.md` after implementation:

- Mark lacune 9 as `Terminé` when checkpoints, hash validation, conflict reporting, and phase-level `dev` resume are implemented.
- Add a dated lot entry: resume checkpoints completed.

## Testing

Unit tests for `src/checkpoint.rs`:

- constructor creates required identity and resume fields.
- checkpoint serializes with stable top-level keys.
- writing and loading round-trips.
- file hash validation passes when content is unchanged.
- file hash validation detects modified files.
- file hash validation detects missing files.
- invalid JSON returns a readable error.

Orchestrator/command tests where practical:

- resume without `cortex.checkpoint.json` fails before workflow execution.
- resume with unsupported workflow checkpoint fails clearly.
- resume with modified tracked file fails before workflow execution.

Dev workflow tests should use focused helpers or test doubles rather than live provider calls:

- checkpoint after `specs-ready` allows PM to be skipped and `specs.md` to be read from disk.
- checkpoint after `architecture-ready` allows Tech Lead to be skipped and developer phase to become the next action.

## Acceptance Criteria

- Normal `dev` runs write `cortex.checkpoint.json` at stable phase boundaries.
- Interrupted `dev` runs leave a checkpoint that identifies the next action.
- `cortex resume <dir>` uses the checkpoint prompt and phase state, not a generic resume prompt.
- Resume aborts before agent execution if tracked files were changed or removed.
- The checkpoint file redacts known secrets.
- README documents the three Cortex artifacts: checkpoint, run report, and manifest.
- `LACUNES.md` marks lacune 9 as complete after implementation.
