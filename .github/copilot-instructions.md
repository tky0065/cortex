# Copilot Instructions

## What is Cortex

Cortex is an agentic CLI written in Rust that simulates a software development company. Given a natural-language idea, it orchestrates specialized AI agents to produce a deployable Git repository.

- **REPL mode** (default): `cargo run` — slash commands `/start`, `/status`, `/abort`, `/continue`
- **One-shot mode**: `cargo run -- start "<idea>" --auto [--workflow dev|marketing|prospecting|code-review]`
- **Resume**: `cargo run -- resume <project-dir>`
- **Verbose logging**: any subcommand accepts `-v` (writes to `cortex.log`)

## Commands

```bash
cargo build
cargo run
cargo check                   # fast type/borrow check — use frequently
cargo test                    # all tests
cargo test <test_name>        # single test, e.g. `cargo test rejects_unlisted_command`
cargo clippy -- -D warnings   # lint; warnings are errors
cargo fmt
```

## Architecture

```
Orchestrator
  └── Box<dyn Workflow>  (resolved by get_workflow("dev"|"marketing"|…))
        └── runs agents sequentially (or parallel via tokio::spawn + Semaphore)
              └── each agent calls providers::model_for_role() → LLM completion
```

**Event bus**: `TuiEvent` is sent over an `mpsc::UnboundedSender<TuiEvent>` (`TuiSender`) that flows from agents through the orchestrator to the TUI renderer. Every agent must emit `AgentStarted` / `AgentDone` around its LLM call.

**RunOptions** is the single struct threaded through every workflow and agent. It carries: `config`, `tx` (TUI sender), `project_dir`, `cancel` (CancellationToken), `resume_rx`/`resume_tx` (interactive pause), `auto`, `verbose`.

**Dev workflow phases** (the canonical example):
1. CEO → brief (string)
2. PM → `specs.md` (written to `project_dir`)
3. Tech Lead → `architecture.md` (includes `FILES_TO_CREATE` section)
4. Developer workers — parallel, semaphore-bounded, each writes one file
5. QA ↔ Developer loop — capped by `config.limits.max_qa_iterations` (default 5); QA approves with `RECOMMENDATION: APPROVE`
6. DevOps → Dockerfile, docker-compose, git commit

Interactive pauses (when `!opts.auto`) use `tokio::select!` on `wait_for_resume()` + `cancel.cancelled()`.

## Key Conventions

### File-as-memory pattern
Agents never receive the full accumulated transcript. Downstream agents read their inputs from disk (`specs.md`, `architecture.md`) via `FileSystem::read()`. This keeps context windows bounded. Context budget per call is configured in `config.toml` under `[limits]`.

### Adding a new agent to an existing workflow
1. Create `src/workflows/<wf>/agents/<role>.rs` — async `pub fn run(...)` that returns `Result<String>`
2. Load the system prompt with `const PREAMBLE: &str = include_str!("../prompts/<role>.md");`
3. Emit `TuiEvent::AgentStarted` at entry and `TuiEvent::AgentDone` at exit
4. Call `providers::model_for_role("<role>", &options.config)?` for the model name
5. Register the role in `providers::model_for_role()` if it's new

### Adding a new workflow
1. Create `src/workflows/<name>/mod.rs` implementing `#[async_trait] impl Workflow`
2. Register it in `workflows::get_workflow()` match arm
3. Declare `pub mod <name>;` in `workflows/mod.rs`

### Security invariants — do not relax
- **Terminal tool** (`tools/terminal.rs`): validate every command against `ALLOWED_COMMANDS = ["cargo", "go", "npm", "pip", "git", "docker"]` before execution. LLM output is untrusted input.
- **Filesystem tool** (`tools/filesystem.rs`): reject `../` path traversal; all I/O must stay within the project output directory.

### QA loop tracking
Track iteration count in the orchestrator / workflow (`for iteration in 0..max_iterations`), never inside an agent.

### Provider routing
`providers::model_for_role()` maps role strings to model names from `Config`. Unknown roles return an error. Code-review workflow roles (`reviewer`, `security`, `performance`, `reporter`) fall back to the `qa` model.

### Config file
`~/.cortex/config.toml` — loaded once at startup via `Config::load()`, then shared as `Arc<Config>`.

```toml
[provider]
default = "ollama"

[models]
ceo       = "ollama/qwen2.5-coder:32b"
developer = "ollama/qwen2.5-coder:32b"
qa        = "ollama/qwen2.5-coder:14b"

[limits]
max_qa_iterations    = 5
max_tokens_per_call  = 8192
max_parallel_workers = 4
```

### Output directory
Workflow output lands in `./cortex-output/<slugified-prompt>/` relative to the working directory where `cortex` was invoked.
