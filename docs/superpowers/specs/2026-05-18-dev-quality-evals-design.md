# Dev Quality Gate And Evals Design

## Context

`LACUNES.md` identifies two related gaps:

- Lacune 1: Cortex does not define measurable quality criteria for generated `dev` workflow projects.
- Lacune 3: Cortex has no reproducible eval harness for representative prompts and generated outputs.

The `dev` workflow already produces `specs.md`, `architecture.md`, source files, QA review, deployment artifacts, README content, and CI hints. The beta docs correctly frame those outputs as drafts requiring review. This lot turns that review into an explicit quality gate and starts the eval harness without introducing provider-dependent automation.

## Goals

- Define a measurable acceptance matrix for generated `dev` workflow projects.
- Add the first `evals/dev/` structure with representative scenarios.
- Provide a minimal executable checker that validates an existing generated project directory.
- Keep evaluation independent of live LLM providers and token costs.
- Update `LACUNES.md` so completed and partial work is marked accurately.

## Non-Goals

- Do not automatically launch `cortex` from the eval harness.
- Do not call any remote provider or require API keys during evaluation.
- Do not claim semantic correctness of generated software beyond the checks that are actually implemented.
- Do not replace Rust unit tests or CI for Cortex itself.
- Do not support every possible language stack in the first eval lot.

## Approach

Use a two-layer design:

1. Human-readable quality gate documentation in `docs/QUALITY_GATE.md`.
2. Machine-readable eval fixtures under `evals/dev/`.

The first executable checker validates a generated project directory that already exists on disk. This makes the harness useful for manual beta testing and future CI without coupling it to model availability, provider latency, or token spending.

## Files

### `docs/QUALITY_GATE.md`

Document the acceptance matrix for `dev` outputs.

The matrix covers:

- Product artifacts: `specs.md`, `architecture.md`, and task breakdown.
- Runnable project structure: expected source files and stack-appropriate config.
- Build and test checks: stack-specific commands must exist and pass when run manually or by the harness.
- Documentation: README must include prerequisites, setup, run commands, test commands, and generated-output caveats.
- Deployment artifacts: Dockerfile, docker-compose, and CI are required only when appropriate for the project type.
- Security baseline: no hardcoded secrets, no obvious path traversal, no committed local machine paths, no unsafe default credentials.
- Maintainability baseline: no blocking TODOs, no placeholder implementation stubs, no unexplained empty files.

Each criterion is classified as:

- `required`: failing this means the generated project is not acceptable.
- `recommended`: failing this should be reported but does not block a beta eval pass.
- `contextual`: required only when the scenario or stack calls for it.

### `evals/dev/acceptance_matrix.toml`

Provide a structured version of the quality gate.

Each check includes:

- `id`
- `name`
- `severity`
- `description`
- `applies_to`
- `manual_review`

The first version should favor simple checks that can be reused across scenarios. Checks that require human judgment are marked with `manual_review = true` instead of being silently ignored.

### `evals/dev/scenarios/*.toml`

Add three initial scenarios:

- `rust_json_cli.toml`: small Rust CLI that validates JSON files.
- `python_file_tool.toml`: simple Python CLI file utility.
- `http_api_minimal.toml`: small HTTP API with tests and README commands.

Each scenario includes:

- Stable scenario id.
- Prompt text.
- Project class and expected stack.
- Required files.
- Optional files.
- Commands to run if matching files exist.
- Scenario-specific acceptance notes.

### `evals/check_dev_output.sh`

Add a minimal shell checker.

The checker accepts:

```bash
evals/check_dev_output.sh <generated-project-dir> [scenario-file]
```

Behavior:

- Fails if the project directory does not exist.
- Fails if `specs.md`, `architecture.md`, or `README.md` are missing.
- If a scenario file is provided, verifies the scenario's required files.
- Reports blocking placeholder patterns such as `TODO: implement`, `TBD`, `placeholder`, and `lorem ipsum` in generated source and docs.
- Detects likely hardcoded secrets using conservative patterns for API keys, tokens, passwords, and private keys.
- Runs stack commands only when they are listed in the scenario and the command binary appears available.
- Prints a compact PASS/FAIL report with check ids.

The checker should be intentionally conservative. It should not delete files, mutate generated projects, install dependencies, or run arbitrary commands from model output.

## Data Flow

1. A beta tester runs Cortex manually and gets a generated project directory.
2. The tester chooses the closest scenario fixture, or runs the generic quality gate only.
3. The checker loads the optional scenario fixture.
4. The checker evaluates filesystem presence, placeholder patterns, secret patterns, and declared stack commands.
5. The checker exits non-zero for required failures and zero for pass or recommended-only findings.

## Error Handling

- Missing project directory: fail with usage guidance.
- Missing scenario file: fail before running checks.
- Malformed scenario file: fail with the line or command that could not be parsed.
- Missing optional command binary: report as skipped unless the scenario marks it required.
- Command failure: fail and print the command name plus captured status.
- Unknown scenario keys: warn but continue, so fixtures can evolve without breaking older checkers.

## Security Constraints

- The checker must not execute commands extracted from generated project files.
- Scenario command lists are repository-owned fixtures, not model output.
- The checker must not print matched secret values; it should print file paths and check ids only.
- The checker must not modify the generated project directory.

## Testing

Verification for this lot:

- Render/read `docs/QUALITY_GATE.md`.
- Parse or inspect the TOML fixtures.
- Run `evals/check_dev_output.sh` against a temporary passing fixture project.
- Run it against a temporary failing fixture project with a missing README or placeholder to prove non-zero failure.
- Confirm no Rust source files are changed.

## `LACUNES.md` Updates

After implementation:

- Mark lacune 1 as `Terminé` with proof pointing to `docs/QUALITY_GATE.md` and `evals/dev/acceptance_matrix.toml`.
- Mark lacune 3 as `En cours` with proof pointing to `evals/dev/` and the initial checker.
- Add a lot entry noting that the first quality gate and minimal eval harness were added.

Do not mark lacune 3 as complete until Cortex can run a broader representative scenario set with scoring and regression tracking.

## Acceptance Criteria

- `docs/QUALITY_GATE.md` exists and clearly defines measurable `dev` acceptance criteria.
- `evals/dev/acceptance_matrix.toml` exists and maps the quality gate to structured checks.
- At least three `evals/dev/scenarios/*.toml` files exist.
- `evals/check_dev_output.sh` validates an existing generated project directory without launching Cortex.
- Verification proves both passing and failing checker behavior.
- `LACUNES.md` accurately marks lacune 1 complete and lacune 3 in progress.
- No Rust runtime behavior changes are included in this lot.
