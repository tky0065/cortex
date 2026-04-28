# Cortex

> **Your entire team, in one command.**

Cortex is an agentic CLI written in Rust that simulates a full software development company. You give it a natural-language idea; it orchestrates specialized AI agents to produce a complete, deployable project.

---

## Table of Contents

1. [Installation](#1-installation)
2. [Quick Start](#2-quick-start)
3. [Configuration](#3-configuration)
4. [Usage Modes](#4-usage-modes)
   - [REPL (interactive)](#41-repl-interactive)
   - [One-shot CLI](#42-one-shot-cli)
   - [Resume an interrupted run](#43-resume-an-interrupted-run)
5. [Web Search](#5-web-search)
6. [Workflows](#6-workflows)
   - [dev](#61-dev--software-development)
   - [marketing](#62-marketing--content-campaign)
   - [prospecting](#63-prospecting--freelance-outreach)
   - [code-review](#64-code-review--code-audit)
7. [Providers & Models](#7-providers--models)
8. [Architecture Internals](#8-architecture-internals)
9. [Security & Sandboxing](#9-security--sandboxing)
10. [Verbose Logging](#10-verbose-logging)
11. [Running Tests](#11-running-tests)
12. [Output Structure](#12-output-structure)

---

## 1. Installation

### Prerequisites

| Requirement | Version |
|-------------|---------|
| Rust toolchain | 1.80+ (edition 2021) |
| Ollama | any recent release (for local models) |
| cargo | comes with Rust |

```bash
# Clone the repository
git clone <repo-url> && cd cortex

# Build (release)
cargo build --release

# The binary lands at:
./target/release/cortex

# Or run directly in development:
cargo run
```

---

## 2. Quick Start

```bash
# Launch the interactive REPL
cargo run

# Inside the REPL, start a software project:
/start dev "build a REST API for a todo-list app in Rust"

# One-shot from the terminal:
cargo run -- start "build a CLI password manager in Go" --auto --workflow dev
```

The generated project appears under `./cortex-output/<slugified-idea>/`.

---

## 3. Configuration

Cortex reads `~/.cortex/config.toml` at startup. If the file does not exist it is **created automatically** with sensible defaults.

```toml
[provider]
default = "ollama"

[models]
ceo       = "ollama/qwen2.5-coder:32b"
pm        = "ollama/qwen2.5-coder:32b"
tech_lead = "ollama/qwen2.5-coder:32b"
developer = "ollama/qwen2.5-coder:32b"
qa        = "ollama/qwen2.5-coder:14b"
devops    = "ollama/qwen2.5-coder:14b"

[limits]
max_qa_iterations    = 5   # how many QA→Developer fix cycles before proceeding
max_tokens_per_call  = 8192
max_parallel_workers = 4   # concurrent developer/profiler workers

[tools]
web_search_enabled = false  # set to true or use /websearch enable in the REPL

[api_keys]
# openrouter = "sk-or-..."
# groq       = "gsk_..."
# together   = "tg-..."
# web_search = "BSA..."    # Brave Search API key
```

### Model string format

Every model value follows `"<provider>/<model-name>"`. Supported providers:

| Prefix | Backend | Env var required |
|--------|---------|------------------|
| `ollama/` | Local Ollama instance | — |
| `openrouter/` | OpenRouter API | `OPENROUTER_API_KEY` |
| `groq/` | Groq API | `GROQ_API_KEY` |
| `together/` | Together AI | `TOGETHER_API_KEY` |

If no prefix is given, `ollama` is assumed.

**Example — mix providers per role:**

```toml
[models]
ceo       = "openrouter/anthropic/claude-3-opus"
developer = "ollama/qwen2.5-coder:32b"
qa        = "groq/llama3-70b-8192"
```

### Setting API keys

API keys can be set in two ways:

**Option A — REPL command (recommended, persisted to `~/.cortex/config.toml`):**

```
/apikey openrouter sk-or-...
/apikey groq       gsk_...
/apikey together   tg-...
/apikey web_search BSA...
```

**Option B — environment variables (session-only):**

```bash
export OPENROUTER_API_KEY="sk-or-..."
export GROQ_API_KEY="gsk_..."
export TOGETHER_API_KEY="tg-..."
export WEB_SEARCH_API_KEY="BSA..."
```

---

## 4. Usage Modes

### 4.1 REPL (interactive)

```bash
cargo run          # or: ./cortex
```

A full-screen TUI opens. Type slash commands in the input bar at the bottom.

| Command | Description |
|---------|-------------|
| `/start <workflow> "<idea>"` | Launch a workflow |
| `/run <workflow> "<prompt>"` | Alias for `/start` |
| `/resume <project-dir>` | Resume an interrupted workflow |
| `/status` | Show whether a workflow is running |
| `/abort` | Cancel the running workflow at the next checkpoint |
| `/continue` | Resume an interactive pause |
| `/config` | Display active config values |
| `/model [<role> <model>]` | Show or change a role's model |
| `/provider [<name>]` | Show or change the default provider |
| `/apikey <provider> <key>` | Set an API key (openrouter / groq / together / web_search) |
| `/websearch [enable\|disable]` | Toggle web search context injection for all agents |
| `/logs` | Toggle log panel focus |
| `/quit` or `/exit` | Exit Cortex |

**Examples:**

```
/start dev "e-commerce backend with Stripe integration"
/start marketing "launch campaign for SaaS productivity app"
/start prospecting "find Python freelancers in Paris"
/start code-review ./my-project
```

The workflow name can be omitted (defaults to `dev`):

```
/start "build a chat app"
```

### 4.2 One-shot CLI

```bash
# Fully autonomous (no interactive pauses)
cortex start "build a Discord bot in Python" --auto --workflow dev

# Interactive (pauses after specs + architecture)
cortex start "build a REST API" --workflow dev

# Marketing workflow
cortex start "launch campaign for my SaaS" --auto --workflow marketing

# Code review
cortex run --workflow code-review ./my-project

# Verbose (writes all agent I/O to cortex.log)
cortex -v start "build a todo app" --auto
```

### 4.3 Resume an interrupted run

```bash
# CLI
cortex resume ./cortex-output/build-a-todo-app

# REPL
/resume ./cortex-output/build-a-todo-app
```

Cortex re-runs the dev workflow with a prompt that asks the agents to continue from the existing files in the directory. Best used when a run was aborted mid-way.

---

## 5. Web Search

When enabled, every agent automatically enriches its prompt with live web search results before calling the LLM. This lets agents use up-to-date information: latest library versions, recent CVEs, current pricing, new best practices, etc.

### How it works

Cortex uses the **[Brave Search API](https://brave.com/search/api/)** (free tier available). When a workflow agent fires, Cortex extracts a search query from the first ~200 characters of the agent's input, fetches the top 5 results, and appends them as a Markdown block to the prompt:

```
## Web Search Results
1. **Title** (https://...)
   Snippet...
2. ...
```

All 18 agent system prompts include a `## Web Search` instruction that tells agents to prefer these results over their training data when relevant.

### Setup

**Step 1 — Get a Brave Search API key:**
Sign up at [brave.com/search/api](https://brave.com/search/api/). The free tier includes 2,000 queries/month.

**Step 2 — Store the key (REPL):**
```
/apikey web_search <your-key>
```

**Step 3 — Enable web search:**
```
/websearch enable
```

Or via `~/.cortex/config.toml`:
```toml
[tools]
web_search_enabled = true

[api_keys]
web_search = "BSA..."
```

### Toggle on / off

```
/websearch enable    # turns on, saves to config
/websearch disable   # turns off, saves to config
/websearch           # prints current status
```

### Offline / no-key mode

If web search is enabled but no API key is set (or the key is empty), the agent runs normally without any web context — no errors are raised.

---

## 6. Workflows

### 6.1 `dev` — Software Development

The flagship workflow. Simulates a complete dev team from idea to deployable repo.

```
User idea
    │
    ▼
[CEO]          Interprets the idea → executive brief
    │
    ▼
[PM]           Writes specs.md (requirements, user stories, acceptance criteria)
    │          ── Interactive pause (if not --auto) ──
    ▼
[Tech Lead]    Writes architecture.md (stack, component diagram, FILES_TO_CREATE list)
    │          ── Interactive pause ──
    ▼
[Developer ×N] Parallel workers (semaphore-bounded), each writes one source file
    │          ── Interactive pause ──
    ▼
[QA ↔ Dev]    QA reads all files → report; Developer fixes flagged files (loop, max 5×)
    │
    ▼
[DevOps]       Generates Dockerfile, docker-compose.yml, git commit
    │
    ▼
Output: ./cortex-output/<project-name>/
```

**Output files:**

| File | Producer |
|------|----------|
| `specs.md` | PM |
| `architecture.md` | Tech Lead |
| `src/*.rs` (or any language) | Developer workers |
| `Dockerfile` / `docker-compose.yml` | DevOps |

**Interactive pauses** (only without `--auto`): the workflow stops after specs, architecture, and the first code generation. Type `/continue` to proceed or `/abort` to stop.

**QA loop:** QA approves by including `RECOMMENDATION: APPROVE` in its report. The loop is hard-capped at `max_qa_iterations` (default 5) regardless.

---

### 6.2 `marketing` — Content Campaign

Produces a full marketing campaign from a product/service description.

```
[Strategist]   → strategy.md  (positioning, target audience, messaging)
       │
       ├─── [Copywriter]  → posts.md   (blog posts, social captions, ads) ─┐
       │                                                                     │ (parallel)
       └─── [Analyst]     → metrics.md (KPIs, success metrics)           ──┘
                │
                ▼
       [Social Media Mgr] → calendar.md (post schedule per channel)
```

**Output directory:** `./cortex-output/marketing-campaign/`

| File | Content |
|------|---------|
| `strategy.md` | Campaign strategy & positioning |
| `posts.md` | All copy (blog, social, ads) |
| `metrics.md` | KPIs and measurement plan |
| `calendar.md` | Publication calendar |

---

### 6.3 `prospecting` — Freelance Outreach

Automates the identification and outreach process for freelance prospects.

> ⚠ **RGPD notice:** Only public data is used. Email sending is dry-run by default (no emails are actually sent).

```
[Researcher]      → prospects.md  (list of matching companies/contacts)
       │
       ├─── [Profiler ×N]   → profiles/<name>.md  (enriched per-prospect profile) ─┐
       │                                                                              │ (parallel)
       └─── [Copywriter ×N] → emails/<name>.md    (personalised email draft)     ──┘
                │
                ▼
       [Outreach Mgr]  → outreach_report.md  (sequencing + follow-up strategy)
```

**Output directory:** `./cortex-output/prospecting-campaign/`

**Optional profile.toml:** Create a `profile.toml` in the project directory to inject your professional background into the researcher's prompt:

```toml
# profile.toml (optional)
name       = "Alice Martin"
stack      = ["Rust", "TypeScript", "Kubernetes"]
experience = "5 years backend, 2 years DevOps"
rate       = "€600/day"
```

---

### 6.4 `code-review` — Code Audit

Runs a multi-angle audit on an existing codebase.

```bash
cortex run --workflow code-review ./my-project
# or in REPL:
/start code-review ./my-project
```

```
[Reviewer]      → review_notes.md     (general quality, style, maintainability)
       │
       ├─── [Security]    → (security issues) ─┐
       │                                         │ (parallel)
       └─── [Performance] → (perf issues)     ──┘
                │
                ▼
       [Reporter]  → code_review_report.md  (consolidated final report)
```

**Output directory:** `./cortex-output/code-review-output/`

Scanned file types: `.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, `.go`, `.java`, `.kt`, `.swift`, `.c`, `.cpp`, `.h`

Skipped directories: `target/`, `node_modules/`, `.git/`, `dist/`, `build/`, `.next/`

Files larger than 8 KB are automatically truncated to protect context windows.

---

## 7. Providers & Models

### Role → Model mapping

All workflows share the same routing function. Roles that do not have a dedicated model fall back to a related one:

| Role | Config key used |
|------|----------------|
| `ceo` | `models.ceo` |
| `pm` | `models.pm` |
| `tech_lead` | `models.tech_lead` |
| `developer` | `models.developer` |
| `qa` | `models.qa` |
| `devops` | `models.devops` |
| `reviewer`, `security`, `performance`, `reporter` | `models.qa` (fallback) |
| `strategist`, `copywriter`, `analyst`, `social_media_manager` | `models.developer` (fallback) |
| `researcher`, `profiler`, `outreach_manager` | `models.developer` (fallback) |

To assign a dedicated model to a role that currently falls back, add the model string to the appropriate config key (custom config keys are not yet supported — edit the `model_for_role()` function in `src/providers/mod.rs`).

### Provider client selection

The `providers::complete(model_str, preamble, prompt)` function parses the prefix of the model string and selects the correct rig client:

```
"ollama/qwen2.5-coder:32b"  → rig_ollama::Client (local, no API key)
"openrouter/gpt-4o"          → rig_openrouter::Client (OPENROUTER_API_KEY)
"groq/llama3-70b-8192"       → groq::Client (GROQ_API_KEY)
"together/mistralai/Mixtral" → together::Client (TOGETHER_API_KEY)
```

---

## 8. Architecture Internals

```
main.rs
 └── Orchestrator
       └── Box<dyn Workflow>   ← resolved by get_workflow("dev" | "marketing" | …)
             └── agents (async fns)
                   └── providers::complete() → LLM call
```

### Key components

#### `Orchestrator` (`src/orchestrator.rs`)
- Holds the `CancellationToken` (for `/abort`) and the resume `mpsc` channel (for `/continue`).
- Creates `RunOptions` and calls `workflow.run(prompt, options)`.
- In verbose mode: spawns a log-tee task that writes all `TokenChunk` events to `cortex.log`.

#### `RunOptions` (`src/workflows/mod.rs`)
Threaded through every workflow and agent. Contains:

| Field | Purpose |
|-------|---------|
| `config` | Shared `Arc<Config>` |
| `tx` | TUI event sender |
| `project_dir` | Sandboxed output directory |
| `cancel` | `CancellationToken` — check `cancel.is_cancelled()` |
| `resume_tx` / `resume_rx` | mpsc pair for interactive pauses |
| `auto` | `true` = skip interactive pauses |
| `verbose` | `true` = log to `cortex.log` |

#### TUI event bus (`src/tui/events.rs`)
All agent output flows through `TuiEvent` over an `mpsc::UnboundedSender`:

| Event | When |
|-------|------|
| `WorkflowStarted` | Workflow begins |
| `AgentStarted { agent }` | Agent begins its LLM call |
| `TokenChunk { agent, chunk }` | Agent produces a text fragment |
| `AgentDone { agent }` | Agent finishes |
| `PhaseComplete { phase }` | A phase milestone is reached |
| `InteractivePause { message }` | Workflow is waiting for `/continue` |
| `Resume` | `/continue` was received |
| `Error { agent, message }` | An error occurred |

#### File-as-memory pattern
Agents never receive the full accumulated transcript. Each downstream agent reads only its required input from disk (`specs.md`, `architecture.md`). This keeps context windows bounded and predictable.

#### Parallel workers
The developer phase (and prospecting's profiler/copywriter phase) uses `tokio::spawn` + `Arc<Semaphore>` to run up to `max_parallel_workers` workers concurrently. Each worker writes its output file immediately to disk.

#### Interactive pauses
In non-auto mode the dev workflow pauses three times via `tokio::select!` on the resume channel:

```
Wait (resume_rx.recv())  or  Wait (cancel.cancelled())
```

The REPL's `/continue` sends `()` to `resume_tx`, unblocking the channel receive.

---

## 9. Security & Sandboxing

### Filesystem sandbox
All file I/O is mediated through `FileSystem` (`src/tools/filesystem.rs`).

- Paths are checked **before** and **after** normalization.
- Any path containing `../`, an absolute root, or a Windows drive prefix is rejected immediately.
- All writes stay inside the workflow's `project_dir`.

### Terminal allowlist
The terminal tool (`src/tools/terminal.rs`) validates every command against a hardcoded allowlist before execution:

```
cargo  go  npm  pip  git  docker
```

LLM output is treated as untrusted input. Any command not on this list is rejected.

### Provider API keys
API keys are read from environment variables only — never stored in config files or source code.

---

## 10. Verbose Logging

Add `-v` to any command to write full agent I/O to `cortex.log` in the working directory:

```bash
cargo run -- -v start "build a CLI tool" --auto
cargo run -- -v resume ./cortex-output/my-project
```

The log file is appended (not overwritten) and each session is marked with a Unix timestamp header.

---

## 11. Running Tests

```bash
cargo test                          # all 64 tests
cargo test <test_name>              # single test, e.g. cargo test parse_model_with_prefix
cargo clippy -- -D warnings         # lint (warnings treated as errors)
cargo fmt                           # format
cargo check                         # fast type/borrow check (< 2s after first build)
```

Test coverage areas:

| Module | Tests |
|--------|-------|
| `providers` | `parse_model_*`, `model_for_role_*` |
| `tools::filesystem` | path traversal, read/write roundtrip |
| `tools::terminal` | allowlist enforcement |
| `tools::web_search` | empty query, offline stub |
| `orchestrator` | event ordering, parallel delivery, lifecycle |
| `tui::widgets` | headless rendering (pipeline, agent panel, logs, input) |

---

## 12. Output Structure

All output lands under `./cortex-output/` relative to the directory where `cortex` was run.

### dev workflow

```
cortex-output/
└── build-a-todo-rest-api/      ← slugified from your idea
    ├── specs.md
    ├── architecture.md
    ├── src/
    │   ├── main.rs
    │   └── ...
    ├── Dockerfile
    └── docker-compose.yml
```

### marketing workflow

```
cortex-output/
└── marketing-campaign/
    ├── strategy.md
    ├── posts.md
    ├── metrics.md
    └── calendar.md
```

### prospecting workflow

```
cortex-output/
└── prospecting-campaign/
    ├── prospects.md
    ├── profiles/
    │   ├── alice-martin.md
    │   └── bob-jones.md
    ├── emails/
    │   ├── alice-martin.md
    │   └── bob-jones.md
    └── outreach_report.md
```

### code-review workflow

```
cortex-output/
└── code-review-output/
    ├── review_notes.md
    ├── code_review_report.md
    └── (security + performance data embedded in report)
```
