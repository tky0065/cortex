# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commit conventions

Never add `Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>` (or any Claude co-author line) to commit messages.

## What is Cortex

Cortex is an agentic CLI written in Rust that simulates a full software development company. Given a single natural-language idea, it orchestrates six specialized AI agents (CEO → PM → Tech Lead → Developer → QA → DevOps) to produce a complete, deployable Git repository.

It runs like `claude` or `opencode`: a REPL by default (`cortex`), with slash commands (`/start`, `/status`, `/abort`), and a one-shot mode (`cortex start "<idea>" --auto`).

Full product specification: `product-development/current-feature/PRD.md`

## Commands

```bash
cargo build                    # compile
cargo run                      # run the CLI
cargo check                    # fast type/borrow check (use this often)
cargo test                     # run all tests
cargo test <test_name>         # run a single test
cargo clippy -- -D warnings    # lint (treat warnings as errors)
cargo fmt                      # format
```

## Implementation Status

| Phase | Tasks | Status |
|-------|-------|--------|
| Phase 1 — Foundations (Cargo + CLI skeleton) | 1, 2, 4 | ✅ Done |
| Phase 5 — Multi-workflow infrastructure | 15, 16, 17 | ✅ Done |
| Phase 1 — Task 3: repl.rs slash commands | 3 | ✅ Done |
| Phase 10 — TUI foundations | 33, 34, 38, 39 | ✅ Done |
| Phase 10 — TUI widgets (pipeline, agent panels, logs) | 35, 36, 37 | ✅ Done |
| Phase 2 — MCP Tools (filesystem + terminal) | 5, 6 | ✅ Done |
| Phase 3 — Context management | 8, 9 | ✅ Done |
| Phase 4 — Providers (Ollama + model assignment) | 11, 14 | ✅ Done |
| Phase 9 — Orchestrator event bus + RunOptions | 22–28 | ✅ Done |
| Phase 6 — Dev workflow agents (all 6) + prompts | 19–25 | ✅ Done |
| Phase 7 — Marketing workflow agents | 26–33 | ✅ Done |
| Phase 8 — Prospecting workflow agents | 34–43 (excl. 40) | ✅ Done |
| Task 40 — tools/email.rs (SMTP dry-run) | 40 | ✅ Done |
| Task 12 — providers/openrouter.rs | 12 | ✅ Done |
| Task 18 — TUI dynamic per-workflow display | 18 | ✅ Done |
| Tasks 30–32 — interactive/auto/abort + CancellationToken | 30, 31, 32 | ✅ Done |
| Tasks 43–44 — summary screen + status bar widgets | 43, 44 | ✅ Done |
| Task 47 — orchestrator event bus tests | 47 | ✅ Done |
| Task 7 — tools/web_search.rs | 7 | ✅ Done |
| Task 10 — context/compressor.rs | 10 | ✅ Done |
| Task 41 — TUI multi-worker parallel display | 41 | ✅ Done |
| Task 42 — interactive pause popup | 42 | ✅ Done |
| Task 48 — ratatui headless tests | 48 | ✅ Done |
| Tasks 13, 40 — Groq/Together providers + token progress | 13, 40 | ✅ Done |
| Tasks 49, 50, 51 — resume + verbose + CI config | 49, 50, 51 | ✅ Done |
| Web search config + agent injection + REPL commands | — | ✅ Done |

Files currently implemented:
- `src/main.rs`, `src/config.rs`, `src/orchestrator.rs`, `src/repl.rs`
- `src/workflows/{mod,dev/mod,marketing/mod,prospecting/mod}.rs`
- `src/workflows/dev/agents/{mod,ceo,pm,tech_lead,developer,qa,devops}.rs`
- `src/workflows/dev/prompts/{ceo,pm,tech_lead,developer,qa,devops}.md`
- `src/workflows/marketing/agents/{mod,strategist,copywriter,analyst,social_media_manager}.rs`
- `src/workflows/marketing/prompts/{strategist,copywriter,analyst,social_media_manager}.md`
- `src/workflows/prospecting/agents/{mod,researcher,profiler,copywriter,outreach_manager}.rs`
- `src/workflows/prospecting/prompts/{researcher,profiler,copywriter,outreach_manager}.md`
- `src/tui/{mod,layout,events}.rs`
- `src/tui/widgets/{mod,input,pipeline,agent_panel,logs}.rs`
- `src/tools/{mod,filesystem,terminal}.rs`
- `src/providers/{mod,ollama}.rs`
- `src/context/mod.rs`
- `src/context/mod.rs`

---

## Architecture

Current and planned `src/` structure:

```
src/
  main.rs                    # ✅ CLI entry: clap + routing --workflow <name>
  repl.rs                    # ✅ Slash commands dispatcher (/start, /status, /abort, /help, /config, /logs, /apikey, /websearch)
  config.rs                  # ✅ ~/.cortex/config.toml — models, limits, profile
  orchestrator.rs            # ✅ Generic orchestrator over dyn Workflow
  tui/
    mod.rs                   # ✅ Tui struct: setup/teardown, async event loop (tokio select!)
    layout.rs                # ✅ 4-zone layout: pipeline, agents, logs, input, status
    events.rs                # ✅ TuiEvent channel (AgentStarted, TokenChunk, AgentDone…)
    widgets/
      mod.rs                 # ✅
      pipeline.rs            # ✅ Dynamic agent status bar (✓ ● ◌ ✗)
      agent_panel.rs         # ✅ Per-agent live block + progress bar
      logs.rs                # ✅ Scrollable timestamped log panel
      input.rs               # ✅ tui-input command bar with cursor
  workflows/
    mod.rs                   # ✅ Workflow trait + get_workflow() registry
    dev/                     # Software development workflow
      mod.rs                 # ✅ DevWorkflow stub
      agents/{ceo, pm, tech_lead, developer, qa, devops}.rs
      prompts/<role>.md
    marketing/               # Marketing & content workflow
      mod.rs                 # ✅ MarketingWorkflow stub
      agents/{strategist, copywriter, social_media_manager, analyst}.rs
      prompts/<role>.md
    prospecting/             # Freelance outreach workflow
      mod.rs                 # ✅ ProspectingWorkflow stub
      agents/{researcher, profiler, copywriter, outreach_manager}.rs
      prompts/<role>.md
  tools/
    mod.rs
    filesystem.rs            # Sandboxed read/write (path-traversal blocked)
    terminal.rs              # Allowlisted command execution
    web_search.rs            # Brave Search API — fetch_context() injects results into every agent prompt
    email.rs                 # SMTP tool — dry-run by default, --send explicit
  context/
    mod.rs                   # Selective context injection per agent
    compressor.rs            # Summary compression for long outputs
  providers/
    mod.rs
    ollama.rs                # Local Ollama (OpenAI-compat)
    openrouter.rs            # Remote fallback
```

## Key Dependencies

| Crate | Version | Role |
|-------|---------|------|
| `rig-core` | 0.35 | Agent construction, LLM provider abstraction |
| `tokio` | 1 (full) | Async runtime |
| `clap` | 4 (derive) | CLI argument parsing |
| `ratatui` | 0.30 | Full TUI framework — layout, widgets, rendering |
| `crossterm` | 0.29 | Cross-platform terminal backend for ratatui |
| `tui-input` | 0.10 | Input widget for the REPL command bar |
| `serde` | 1 (derive) | Serialization |
| `toml` | 0.8 | Config file parsing |
| `anyhow` | 1 | Error handling |
| `dirs` | 5 | Home directory resolution |
| `async-trait` | 0.1 | Async fn in `dyn Workflow` trait objects |

## Critical Design Constraints

**Context window safety**: Never pass the full accumulated transcript to downstream agents. Each agent reads only its required input documents from disk (`specs.md`, `architecture.md`) — this is the file-as-memory pattern. Context budget per agent call is configurable in `config.toml`.

**Terminal tool security**: The terminal MCP tool must validate every command against a hardcoded allowlist (`cargo`, `go`, `npm`, `pip`, `git`, `docker`) before execution. Commands are built from LLM output — treat them as untrusted input.

**Filesystem sandbox**: All file reads/writes are restricted to the project output directory. Validate paths to reject `../` traversal before any `std::fs` call.

**QA ↔ Developer loop**: Has a hard cap (`max_qa_iterations`, default 5). Track iteration count in the orchestrator state, not inside the agents.

## Agent Workflow

```
User idea → [CEO] → [PM: specs.md] → [Tech Lead: architecture.md]
         → [Developer: source files] ↔ [QA: tests + bug reports] (loop)
         → [DevOps: Dockerfile, docker-compose, git commit]
         → Final report
```

In `--interactive` mode the workflow pauses after `specs.md`, `architecture.md`, and the first passing build for user validation.

## Web Search

When `config.tools.web_search_enabled = true` and `api_keys.web_search` is set, every call to `providers::complete()` automatically:

1. Extracts the first ~200 chars of the agent's `prompt` as a search query
2. Calls `tools::web_search::fetch_context(query, config)` (Brave Search API)
3. Appends a `## Web Search Results` Markdown block to the prompt before the LLM call

All 18 agent system prompts include a `## Web Search` instruction to use these results.

**REPL commands:**
```
/apikey web_search <key>   # store Brave Search API key in config
/websearch enable          # activate (saved to ~/.cortex/config.toml)
/websearch disable         # deactivate
/websearch                 # show current status
```

**Injection point:** `src/providers/mod.rs` → `complete()` — no individual agent files need to change when adding web search to a new workflow.

## Configuration File

User config lives at `~/.cortex/config.toml`:

```toml
[provider]
default = "ollama"

[models]
ceo       = "ollama/qwen2.5-coder:32b"
developer = "ollama/qwen2.5-coder:32b"
qa        = "ollama/qwen2.5-coder:14b"

[limits]
max_qa_iterations = 5
max_tokens_per_call = 8192

[tools]
web_search_enabled = false  # enable with /websearch enable in the REPL

[api_keys]
# web_search = "BSA..."     # Brave Search API key — set with /apikey web_search <key>
```
