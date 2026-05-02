# Cortex

> **Your entire team, in one command.**

Cortex is a beta agentic CLI written in Rust that simulates a full software development company. You give it a natural-language idea; it orchestrates specialized AI agents to produce a complete, deployable project.

**Status:** Beta. Cortex is ready for early adopters, but workflows, providers, and generated project structure may still evolve before a stable 1.0 release.

## What's new in 0.1.8 beta

- **Execution Modes** — Cycle through four modes with **Shift+Tab** (just like Claude Code). The active mode is always visible in the status bar with a colour-coded badge.
  - **NORMAL** (default) — interactive pauses at key checkpoints (specs, architecture, first build).
  - **PLAN** — the Planner agent scans the project, asks 2–3 clarifying questions, and produces `PLAN.md`. A full-screen review popup then lets you **Approve**, **Edit inline**, or **Abort** before any code is written. Chat also enters plan-only mode — no files are written until you approve.
  - **AUTO** — all agents run without any interactive pauses (equivalent to `--auto`).
  - **REVIEW** — pauses before every agent phase for manual confirmation.
- **PlanReview popup** — full-screen overlay with scrollable PLAN.md preview, Markdown syntax highlighting, inline editor (toggle with `e`), and an action bar (`[Enter] Approve · [e] Edit · [Esc] Abort`).
- **`/mode` command** — show or change the current execution mode from the REPL (`/mode plan`, `/mode auto`, etc.).
- **`/approve` command** — alias for `/continue`; confirms the plan or unblocks any Review-mode pause.

## What's new in 0.1.7 beta

- **Task Management Widget**: A dedicated panel in the TUI now displays the real-time status of all tasks within a workflow, making it easier to track progress at a glance.
- **Workflow Explorer**: Use `/workflows` in the REPL or `cortex workflows` in the CLI to see a list of all available workflows and their detailed descriptions.
- **Responsive Agent Grid**: The TUI now intelligently adapts the agent panel layout to fit smaller terminal windows, ensuring readability even in constrained spaces.
- **Professional Progress Indicators**: Refined visual feedback with better alignment and clearer status icons across all widgets.

## What's new in 0.1.6 beta

- **Inline Mentions**: Type `@` in the TUI to autocomplete project files and folders, or `$` to autocomplete installed skills. Referenced files/folders are injected into model context, and `$skill` forces that skill into the request.
- **Live Diff Viewer**: Every time an agent writes a file, a popup appears automatically showing added lines in green and deleted lines in red — like Claude, Copilot, or Gemini. Navigate multiple files with `n`/`p`, scroll with `j`/`k`, dismiss with `q`/`Esc`.
- **Agent Stream Buffer Cap**: The streaming buffer is capped at 50 000 characters per agent, preventing unbounded memory growth on long runs.
- **Refreshed TUI Style**: Rounded borders on all panels for a cleaner look, new `◐◓◑◒` spinner replacing the braille dots, and a gradient on streamed text (newest line full-bright, older lines progressively dimmer).

## What's new in 0.1.5 beta

- **Beta Release Track**: Cortex 0.1.5 keeps the project on the beta track across the CLI metadata, README, and website.
- **Professional Progress Indicators**: Replaced the bulky green gauge with a discrete inline progress bar (`[███░░] 60%`) and an animated spinner for active agents.
- **Scrollable Agent Panels**: View long reports directly in the TUI using **Alt + Up/Down** or **PageUp/Down**.
- **`cortex init` / `/init`**: Scans the current project and maintains `AGENTS.md` as durable agent context.
- **Project Context Injection**: `AGENTS.md` is automatically injected into workflow agents and assistant prompts.
- **Expanded Provider System**: `/connect` now exposes the full provider registry, including ChatGPT Plus/Pro OAuth, GitHub Copilot device login, GitLab Duo auth, Vertex AI, Bedrock, local providers, hosted APIs, aggregators, and custom OpenAI-compatible endpoints.
- **Provider Streaming Fixes**: ChatGPT Plus/Pro now uses the Codex Responses stream required by the backend; Gemma models on Gemini avoid unsupported developer/system instructions by inlining Cortex instructions into the user prompt.
- **Expanded TUI**: Improved command palette, skill picker, and status widgets.

---

## Table of Contents

1. [Installation](#1-installation)
2. [Updating](#2-updating)
3. [Quick Start](#3-quick-start)
4. [Configuration](#4-configuration)
5. [Usage Modes](#5-usage-modes)
   - [REPL (interactive)](#51-repl-interactive)
   - [One-shot CLI](#52-one-shot-cli)
   - [Initialize project context](#53-initialize-project-context)
   - [Resume an interrupted run](#54-resume-an-interrupted-run)
   - [Execution modes](#55-execution-modes)
6. [Project Context](#6-project-context)
7. [Task Tracking](#7-task-tracking)
8. [Skills](#8-skills)
9. [Web Search](#9-web-search)
10. [Workflows](#10-workflows)
   - [dev](#91-dev--software-development)
   - [marketing](#92-marketing--content-campaign)
   - [prospecting](#93-prospecting--freelance-outreach)
   - [code-review](#94-code-review--code-audit)
11. [Providers & Models](#11-providers--models)
12. [Architecture Internals](#12-architecture-internals)
13. [Security & Sandboxing](#13-security--sandboxing)
14. [Verbose Logging](#14-verbose-logging)
15. [Running Tests](#15-running-tests)
16. [Release Process](#16-release-process)
17. [Output Structure](#17-output-structure)

---

## 1. Installation

### macOS and Linux

Install the latest published Cortex beta release:

```bash
curl -fsSL https://raw.githubusercontent.com/tky0065/cortex/main/install.sh | sh
```

The installer downloads the right binary from GitHub Releases, verifies its SHA-256 checksum, and installs it to `~/.local/bin/cortex`.

If `~/.local/bin` is not on your `PATH`, add it:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Windows

Install the latest Windows beta release from PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/tky0065/cortex/main/install.ps1 | iex"
```

The installer downloads `cortex.exe`, verifies its SHA-256 checksum, installs it to `%USERPROFILE%\.cortex\bin`, and adds that directory to your user `PATH` if needed.

### Runtime prerequisites

| Requirement | When needed |
|-------------|-------------|
| Ollama | Default local provider |
| Provider API key or account auth | Remote model providers |
| Brave Search API key | Optional web search context |

### Build from source

Developers can build Cortex directly from the repository:

```bash
git clone https://github.com/tky0065/cortex.git
cd cortex
cargo build --release
./target/release/cortex --version
```

---

## 2. Updating

Cortex checks for a newer GitHub Release when the TUI starts. If an update exists, it prints a non-blocking log message:

```text
Update available: 0.1.7 -> v0.1.8. Run /update to install.
```

Update from the terminal:

```bash
cortex update --check
cortex update
cortex update --version v0.1.8
```

Update from the REPL:

```text
/update check
/update
/update v0.1.8
```

The updater downloads the matching GitHub Release archive, verifies `SHA256SUMS`, and replaces the current `cortex` binary. On Windows, restart the terminal after updating.

---

## 3. Quick Start

```bash
# Launch the interactive REPL
cortex

# Inside the REPL, start a software project:
/start dev "build a REST API for a todo-list app in Rust"

# One-shot from the terminal:
cortex start "build a CLI password manager in Go" --auto --workflow dev
```

For the `dev` workflow, generated files are written directly into the directory
where `cortex` was launched.

---

## 4. Configuration

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
skills_enabled = true       # inject relevant installed skills into agent prompts
max_skill_context_chars = 12000

[api_keys]
# openai     = "sk-..."
# anthropic  = "sk-ant-..."
# gemini     = "AIza..."
# openrouter = "sk-or-..."
# groq       = "gsk_..."
# together   = "tg-..."
# web_search = "BSA..."    # Brave Search API key
```

OAuth and device-login credentials are stored separately in `~/.cortex/auth.json` by `/connect`.

### Model string format

Every model value follows `"<provider>/<model-name>"`. Supported providers:

| Prefix | Backend | Env var required |
|--------|---------|------------------|
| `ollama/` | Local Ollama instance | — |
| `lmstudio/` | Local LM Studio server | — |
| `openai/` | OpenAI API | `OPENAI_API_KEY` |
| `openai_chatgpt/` | ChatGPT Plus/Pro Codex backend | `/connect openai chatgpt_browser` |
| `anthropic/` | Anthropic API | `ANTHROPIC_API_KEY` |
| `gemini/` | Google Gemini API, including Gemma models | `GEMINI_API_KEY` |
| `mistral/` | Mistral AI | `MISTRAL_API_KEY` |
| `deepseek/` | DeepSeek API | `DEEPSEEK_API_KEY` |
| `xai/` | xAI API | `XAI_API_KEY` |
| `cohere/` | Cohere API | `COHERE_API_KEY` |
| `perplexity/` | Perplexity API | `PERPLEXITY_API_KEY` |
| `huggingface/` | Hugging Face Inference Providers | `HUGGINGFACE_API_KEY` |
| `azure_openai/` | Azure OpenAI | `AZURE_OPENAI_API_KEY` + endpoint env |
| `github_copilot/` | GitHub Copilot subscription backend | `/connect github_copilot github_device` |
| `gitlab_duo/` | GitLab Duo compatible accounts | `/connect gitlab_duo` |
| `google_vertex/` | Google Vertex AI | Google ADC / `gcloud` |
| `amazon_bedrock/` | Amazon Bedrock | AWS credential chain |
| `openrouter/` | OpenRouter API | `OPENROUTER_API_KEY` |
| `groq/` | Groq API | `GROQ_API_KEY` |
| `together/` | Together AI | `TOGETHER_API_KEY` |
| `fireworks/`, `deepinfra/`, `cerebras/`, `moonshot/`, `zai/`, `302ai/`, `alibaba/`, `cloudflare/`, `minimax/`, `nebius/`, `scaleway/`, `vercel_ai_gateway/` | Hosted or aggregator OpenAI-compatible APIs | provider-specific API key |

If no prefix is given, `ollama` is assumed.

Gemma models served through the Gemini API, such as `gemini/gemma-3-12b-it`, do not accept developer/system instructions. Cortex keeps them usable by inlining its instructions into the user prompt while leaving native `gemini-*` models on the standard system-instruction path.

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

Use `/connect` for interactive auth flows and account-based providers:

```text
/connect                         # open the provider picker
/connect openai                  # list OpenAI auth methods
/connect openai chatgpt_browser  # ChatGPT Plus/Pro OAuth browser flow
/connect github_copilot github_device
/connect amazon_bedrock aws_profile
```

**Option B — environment variables (session-only):**

```bash
export OPENROUTER_API_KEY="sk-or-..."
export GROQ_API_KEY="gsk_..."
export TOGETHER_API_KEY="tg-..."
export WEB_SEARCH_API_KEY="BSA..."
```

---

## 5. Usage Modes

### 5.1 REPL (interactive)

```bash
cortex
```

A full-screen TUI opens. Type slash commands in the input bar at the bottom.

| Command | Description |
|---------|-------------|
| `/start <workflow> "<idea>"` | Launch a workflow |
| `/run <workflow> "<prompt>"` | Alias for `/start` |
| `/resume <project-dir>` | Resume an interrupted workflow |
| `/init [--force]` | Scan the current project and generate/update `AGENTS.md` |
| `/status` | Show whether a workflow is running |
| `/abort` | Cancel the running workflow at the next checkpoint |
| `/continue` | Resume an interactive pause |
| `/approve` | Confirm a plan or resume a Review-mode pause (alias for `/continue`) |
| `/mode [<name>]` | Show or set the execution mode (`normal`, `plan`, `auto`, `review`) |
| `/config` | Display active config values |
| `/model [<role> <model>]` | Show or change a role's model |
| `/provider [<name>]` | Show or change the default provider |
| `/connect [provider method]` | Connect provider auth or open the provider picker |
| `/apikey <provider> <key>` | Set an API key |
| `/websearch [enable\|disable]` | Toggle web search context injection for all agents |
| `/skill` / `/skills` | Browse, install, enable, disable, and remove Cortex skills |
| `/update [check\|<version>]` | Check for or install Cortex updates |
| `/focus <agent>` | Show only logs for one agent |
| `/clear` | Clear visible logs |
| `/logs` | Toggle log panel focus |
| `/quit` or `/exit` | Exit Cortex |

**Keyboard Shortcuts**

| Shortcut | Action |
|----------|-------------|
| **Alt + ↑ / ↓** | Scroll active agent content (5 lines) |
| **PageUp / PageDown** | Fast scroll active agent content (15 lines) |
| **↑ / ↓** | Cycle through command history |
| **Shift + Tab** | Cycle execution mode: NORMAL → PLAN → AUTO → REVIEW |
| **Tab** | Trigger command palette or autocomplete `/`, `@`, and `$` suggestions |
| **Ctrl + C** | Abort current action or exit |
| **Ctrl + Y** | Copy all visible logs and agent output to clipboard |

**Examples:**

```
/start dev "e-commerce backend with Stripe integration"
/start marketing "launch campaign for SaaS productivity app"
/start prospecting "find Python freelancers in Paris"
/start code-review ./my-project
explain @src/repl.rs with $rust
/start dev "improve @src/tui/ using $ratatui"
```

The workflow name can be omitted (defaults to `dev`):

```
/start "build a chat app"
```

### 5.2 One-shot CLI

```bash
# Fully autonomous (no interactive pauses)
cortex start "build a Discord bot in Python" --auto --workflow dev

# Interactive (pauses after specs + architecture)
cortex start "build a REST API" --workflow dev

# Marketing workflow
cortex start "launch campaign for my SaaS" --auto --workflow marketing

# Code review
cortex run --workflow code-review ./my-project

# Initialize project context for future Cortex agents
cortex init

# Verbose (writes all agent I/O to cortex.log)
cortex -v start "build a todo app" --auto
```

### 5.3 Initialize project context

```bash
# CLI
cortex init

# REPL
/init
```

`init` scans the current project, detects stack and commands, and generates or updates `AGENTS.md`. If `AGENTS.md` already exists, Cortex preserves manual content and refreshes only the Cortex-managed section between stable markers. Future agents automatically receive this file as project context before planning or changing code.

### 5.4 Resume an interrupted run

```bash
# CLI
cortex resume ./demo

# REPL
/resume ./demo
```

Cortex re-runs the dev workflow with a prompt that asks the agents to continue from the existing files in the directory. Best used when a run was aborted mid-way.

### 5.5 Execution modes

Press **Shift+Tab** in the TUI to cycle the execution mode. The active mode is shown in the status bar at the bottom of the screen.

| Mode | Colour | Behaviour |
|------|--------|-----------|
| **NORMAL** | white | Pauses after specs, architecture, and first build for manual review |
| **PLAN** | yellow | Planner runs first — asks questions, writes `PLAN.md`, shows review popup. `/approve` starts the agents |
| **AUTO** | green | No pauses — equivalent to `--auto` |
| **REVIEW** | orange | Pauses before every agent phase for confirmation |

You can also set or inspect the mode from the REPL:

```
/mode           # show current mode
/mode plan      # switch to Plan mode
/mode auto      # switch to Auto mode
/mode review    # switch to Review mode
/mode normal    # reset to Normal mode
```

**Plan mode flow:**

```
/start dev "build a todo app"    [mode = PLAN]
  → Planner scans project
  → Asks 2–3 clarifying questions (one at a time in the TUI)
  → Writes PLAN.md
  → PlanReview popup opens (scroll ↑↓, edit e, approve Enter, abort Esc)
/approve  or  Enter in the popup
  → CEO → PM → Tech Lead → Developer → QA → DevOps
```

---

## 6. Project Context

`cortex init` prepares an existing or new project for future Cortex changes.

```bash
cortex init
# or inside the REPL:
/init
```

The command scans the current directory, detects stack, manifests, commands, and project layout, then creates or updates `AGENTS.md`. Manual content is preserved; Cortex only refreshes the managed section between:

```md
<!-- cortex:init:start -->
...
<!-- cortex:init:end -->
```

Future workflow agents and the assistant automatically receive the current directory's `AGENTS.md` as durable project context before planning or editing. `init` skips dependency folders, generated artifacts, VCS metadata, agent caches, binary files, and likely secret files. If the LLM returns empty output or provider generation fails, Cortex writes a deterministic fallback section instead of leaving `AGENTS.md` empty.

Inside the TUI input bar, type `@` to autocomplete project files and folders. Mentioned files are injected into the prompt; mentioned folders inject a compact tree and bounded text snippets. Suggestions skip generated, dependency, cache, VCS, and secret-like paths such as `node_modules`, `target`, `.git`, `.env`, and `.venv`.

---

## 7. Task Tracking

Cortex displays a live **Tasks** panel in the TUI that shows the progress of every phase in the current workflow.

### How it works

Tasks come from two sources:

1. **Workflow phases** — each workflow emits phase checkpoints automatically. For example, the `dev` workflow reports: `CEO brief`, `PM specs.md`, `Tech Lead architecture.md`, `Developer files`, `QA loop`, `DevOps packaging`. Each phase is marked done as the corresponding agent finishes.

2. **`TASKS.md` file** — Cortex watches the project directory for a `TASKS.md` file and reloads it every 2 seconds. The file uses GitHub-flavoured Markdown task syntax:

```md
- [ ] Design the database schema
- [x] Write the REST endpoints
- [ ] Add authentication middleware
- [ ] Write integration tests
```

Any file matching this format — created by an agent, by you, or by `PLAN.md` approvals — is immediately reflected in the Tasks panel.

### Tasks panel

The panel header shows a running count: **Tasks (3/7)**. Each item shows:

- `[ ]` in accent colour → pending
- `[x]` in green, struck-through → completed

The panel is visible during any active workflow and clears automatically when the workflow finishes.

---

## 8. Skills


Cortex skills are reusable local instructions that can be installed globally or per project and injected into relevant agent prompts.

```bash
cortex skill list
cortex skill add <owner/repo|path|url> --project
cortex skill enable <name> --project
cortex skill disable <name> --project
cortex skill remove <name> --project
cortex skill create <name> --description "When to use this skill" --project
```

Inside the TUI, `/skill` opens the skill browser/manager and `/skill <subcommand>` runs the same management commands from the REPL. Project-scoped skills are preferred over global skills with the same name. Skill injection is controlled by `tools.skills_enabled` and bounded by `tools.max_skill_context_chars`.

In free-form prompts and workflow prompts, type `$` to autocomplete installed skills. A mention like `$rust` forces that skill into the model context for the request, even if the automatic relevance scoring would not have selected it.

---

## 9. Web Search

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

## 10. Workflows

### 9.1 `dev` — Software Development

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
Output: ./
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

### 9.2 `marketing` — Content Campaign

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

### 9.3 `prospecting` — Freelance Outreach

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

### 9.4 `code-review` — Code Audit

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

## 11. Providers & Models

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
"openai_chatgpt/gpt-5.5"     → ChatGPT Codex Responses stream (OAuth via /connect)
"gemini/gemma-3-12b-it"      → Gemini client with inline instructions fallback
"amazon_bedrock/<model-id>"  → AWS Bedrock Runtime (AWS credential chain)
"openrouter/gpt-4o"          → rig_openrouter::Client (OPENROUTER_API_KEY)
"groq/llama3-70b-8192"       → groq::Client (GROQ_API_KEY)
"together/mistralai/Mixtral" → together::Client (TOGETHER_API_KEY)
```

---

## 12. Architecture Internals

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
| `execution_mode` | `Normal` / `Plan` / `Auto` / `Review` — controls agent pause strategy |
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
| `ModeChanged(mode)` | User cycled execution mode (Shift+Tab or `/mode`) |
| `PlanGenerated { path }` | Planner wrote `PLAN.md` — triggers the PlanReview popup |
| `Error { agent, message }` | An error occurred |

#### Assistant and streamed chat
Free-form REPL messages are routed to the assistant loop. Visible assistant text streams through the TUI while private `<tool_call>` XML is filtered from display, so tool use stays internal and the user sees only natural-language output and command/file operation summaries.

When a prompt includes a direct URL, the assistant can fetch that URL directly without Brave Search. If web search is disabled, it can still use direct fallbacks such as URL fetching, `gh` for GitHub metadata, `git` for local history, and local file search before reporting that live discovery is unavailable.

#### File-as-memory pattern
Agents never receive the full accumulated transcript. Each downstream agent reads only its required input from disk (`specs.md`, `architecture.md`). This keeps context windows bounded and predictable.

When a project-level `AGENTS.md` exists in the current working directory, Cortex also injects it into every agent preamble as durable project context. Generate or refresh it with `cortex init` or `/init`.

#### Parallel workers
The developer phase (and prospecting's profiler/copywriter phase) uses `tokio::spawn` + `Arc<Semaphore>` to run up to `max_parallel_workers` workers concurrently. Each worker writes its output file immediately to disk.

#### Interactive pauses
In non-auto mode the dev workflow pauses three times via `tokio::select!` on the resume channel:

```
Wait (resume_rx.recv())  or  Wait (cancel.cancelled())
```

The REPL's `/continue` sends `()` to `resume_tx`, unblocking the channel receive.

---

## 13. Security & Sandboxing

### Filesystem sandbox
All file I/O is mediated through `FileSystem` (`src/tools/filesystem.rs`).

- Paths are checked **before** and **after** normalization.
- Any path containing `../`, an absolute root, or a Windows drive prefix is rejected immediately.
- All writes stay inside the workflow's `project_dir`.

### Terminal allowlist
The terminal tool (`src/tools/terminal.rs`) validates every command against a hardcoded allowlist before execution:

```
cargo  go  npm  pip  git  docker  gh  curl  jq  rg
```

LLM output is treated as untrusted input. Any command not on this list is rejected.

### Provider API keys
API keys can be read from environment variables or stored locally in `~/.cortex/config.toml` via `/apikey`. OAuth/device/account credentials are stored in `~/.cortex/auth.json` via `/connect`. Never commit keys or auth stores to source control.

---

## 14. Verbose Logging

Add `-v` to any command to write full agent I/O to `cortex.log` in the working directory:

```bash
cortex -v start "build a CLI tool" --auto
cortex -v resume ./demo
```

The log file is appended (not overwritten) and each session is marked with a Unix timestamp header.

---

## 15. Running Tests

```bash
cargo test                          # all tests
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
| `updater` | version comparison, archive naming, checksum parsing |
| `orchestrator` | event ordering, parallel delivery, lifecycle |
| `tui::widgets` | headless rendering (pipeline, agent panel, logs, input) |
| `project_context` | `AGENTS.md` merge, fallback generation, scanner exclusions |
| `skills` | install parsing, scope handling, relevance scoring |

---

## 16. Release Process

Cortex beta releases are published through GitHub Releases.

1. Update `Cargo.toml` to the release version.
2. Run the local checks:

```bash
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt
```

3. Commit the release changes.
4. Create and push a version tag:

```bash
git tag v0.1.7
git push origin main --tags
```

The `.github/workflows/release.yml` workflow builds macOS, Linux, and Windows binaries, publishes archives to the GitHub release, and uploads `SHA256SUMS`. The install scripts always download from the latest GitHub release unless `CORTEX_VERSION` is set.

---

## 17. Output Structure

The `dev` workflow writes generated project files directly into the directory where
`cortex` was run. Other workflows keep their generated artifacts under
`./cortex-output/`.

### dev workflow

```
./
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
