# Beta Readiness Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add short beta-readiness documentation and convert `LACUNES.md` into a trackable backlog with completed documentation/process gaps marked clearly.

**Architecture:** This is a documentation-only change. New docs live under `docs/`, issue reporting uses the existing `.github/ISSUE_TEMPLATE/` Markdown convention, and `README.md` only links out to avoid duplicating long guidance.

**Tech Stack:** Markdown, GitHub issue template Markdown, repository-relative links.

---

## File Structure

- Create `docs/BETA.md`: public beta position, workflow support stance, limits, quick beta path, and failure reporting pointer.
- Create `docs/PROVIDERS.md`: provider support levels, local vs remote trade-offs, model recommendations, cost/privacy/compatibility notes, and troubleshooting.
- Create `.github/ISSUE_TEMPLATE/failed_run.md`: structured failed-run issue template matching existing issue template style.
- Modify `README.md`: add a concise beta resources section linking to the new docs and issue template.
- Modify `LACUNES.md`: add status/proof lines to every numbered lacune and mark only covered docs/process items as `Terminé`.

## Status Mapping For `LACUNES.md`

Mark these as `Terminé` in this lot:

- 4. Positionnement produit trop large pour une beta fiable. Proof: `docs/BETA.md`.
- 5. Strategie provider insuffisamment clarifiee. Proof: `docs/PROVIDERS.md`.
- 10. Documentation d'utilisation avancee incomplete. Proof: `docs/BETA.md` and README links.
- 16. Audience cible trop implicite. Proof: `docs/BETA.md`.
- 18. Pas de strategie de support et feedback beta. Proof: `.github/ISSUE_TEMPLATE/failed_run.md`.
- 19. Promesse "software company" potentiellement trop forte. Proof: `docs/BETA.md`.

Keep all other lacunes as `À faire` because this lot does not implement their runtime behavior or full process.

---

### Task 1: Add Beta Guide

**Files:**
- Create: `docs/BETA.md`

- [ ] **Step 1: Create the beta guide**

Add `docs/BETA.md` with this content:

~~~markdown
# Cortex Beta Guide

Cortex is in beta. The CLI is usable for early adopters, but workflow behavior, provider compatibility, and generated project structure can still change before a stable 1.0 release.

## Recommended Beta Path

Use the `dev` workflow as the flagship beta path:

```bash
cortex start "Build a small Rust CLI that validates JSON files" --auto
```

This path exercises the core Cortex promise: a multi-agent software workflow that turns a product idea into project files, docs, tests, and deployment hints.

## Workflow Support Levels

| Workflow | Beta stance | Use it for | Current limits |
|----------|-------------|------------|----------------|
| `dev` | Flagship beta workflow | Generating small to medium software projects | Output quality depends heavily on provider/model choice and project scope |
| `code-review` | Experimental | First-pass review, security notes, performance notes | Findings need human validation before merge decisions |
| `marketing` | Experimental | Campaign drafts, positioning, content calendars | Copy still needs brand and compliance review |
| `prospecting` | Experimental | Public-data prospect research and outreach drafts | Requires careful human review before any outreach |
| Custom workflows | Advanced experimental | Local workflow experiments and team-specific agents | Invalid definitions can produce confusing runs until validation is stricter |

## What Beta Means

Cortex can produce useful project scaffolds and workflow outputs, but a beta run is not a guarantee of production-ready software. Treat generated repositories as drafts that need review.

Before shipping generated code, verify:

- The project builds from a clean checkout.
- Tests pass locally.
- The README launch commands work.
- No secrets or local paths were written into generated files.
- Docker or deployment files match your actual environment.
- Generated security, marketing, and outreach claims are reviewed by a human.

## Positioning

The phrase "your entire team, in one command" describes the orchestration model: Cortex routes work through specialized agents. In beta, the more precise expectation is:

> Cortex is a local-first, multi-agent CLI for generating and reviewing project work from a high-level prompt.

Use Cortex when you want a structured first pass with files on disk. Use a human review loop when correctness, security, compliance, or production readiness matters.

## Short Onboarding Path

1. Install Cortex from the README.
2. Connect a provider with `/connect` or configure Ollama locally.
3. Run the `dev` workflow on a small, concrete project idea.
4. Inspect generated files, commands, tests, and deployment artifacts.
5. If the run fails, open a failed-run issue with the template in `.github/ISSUE_TEMPLATE/failed_run.md`.

## Good Beta Prompts

Prefer prompts with:

- A small scope.
- A named stack or language.
- Clear acceptance criteria.
- Explicit exclusions.

Example:

```text
Build a Rust CLI named jsonlint that validates JSON files, prints line/column errors, includes unit tests, and ships with a README. Do not add networking or a TUI.
```

Avoid prompts that ask for a whole company platform, production compliance, billing, authentication, analytics, and deployment in one run.
~~~

- [ ] **Step 2: Verify the file renders as plain Markdown**

Run:

```bash
sed -n '1,240p' docs/BETA.md
```

Expected: the file prints without shell errors, and fenced code blocks are balanced.

- [ ] **Step 3: Commit the beta guide**

Run:

```bash
git add docs/BETA.md
git commit -m "docs: add beta guide"
```

Expected: one commit containing only `docs/BETA.md`.

---

### Task 2: Add Provider Guide

**Files:**
- Create: `docs/PROVIDERS.md`

- [ ] **Step 1: Create the provider guide**

Add `docs/PROVIDERS.md` with this content:

~~~markdown
# Cortex Providers Guide

Cortex quality depends heavily on the provider and model selected for each agent role. A provider can be correctly configured and still produce weak results if the model is too small, too slow, rate-limited, or missing features Cortex expects.

## Provider Support Levels

| Level | Meaning | Examples |
|-------|---------|----------|
| Default local | Works without sending prompts to hosted model APIs when Ollama is installed | Ollama |
| Direct hosted | Uses a first-party hosted model API or account auth path | OpenAI-compatible providers, Anthropic, Gemini, Mistral, DeepSeek, xAI, Cohere, Perplexity, Hugging Face, Azure OpenAI |
| Aggregator | Routes through a provider marketplace or gateway | OpenRouter, Together, Groq, Fireworks, DeepInfra, Cerebras, Moonshot, Vercel AI Gateway |
| Custom OpenAI-compatible | User-defined endpoint and model list | Local gateways, self-hosted model routers, internal company endpoints |
| Experimental auth integrations | Available for early testing; behavior can change | ChatGPT Plus/Pro OAuth, GitHub Copilot, GitLab Duo, Vertex AI, Bedrock |

Check the README for the exact commands supported by the current release.

## Local vs Remote

| Choice | Benefits | Trade-offs |
|--------|----------|------------|
| Local provider | Better privacy, predictable local control, no per-token API bill | Requires local hardware, model setup, and may produce weaker results on small models |
| Remote provider | Stronger models, easier setup for many users, better reasoning on complex workflows | Sends prompts and project context to an external service, can hit rate limits or cost more |
| Aggregator | Many models behind one account, easy fallback testing | Pricing, model availability, and tool behavior can vary by route |
| Custom endpoint | Fits internal infrastructure and policy | You own compatibility, auth, latency, and model quality validation |

## Model Recommendations By Workflow

| Workflow class | Recommended model quality | Why |
|----------------|---------------------------|-----|
| `dev` project generation | Strong coding model with reliable instruction following | Needs coherent specs, architecture, source files, tests, and deployment docs |
| `code-review` | Strong reasoning model with code and security ability | Needs precise findings and low false confidence |
| `marketing` | General writing model with good style control | Needs useful drafts, but correctness risk is lower than code generation |
| `prospecting` | Research-capable model with careful instruction following | Needs grounded summaries and conservative outreach drafts |
| Custom workflows | Match model quality to the riskiest agent in the workflow | One weak agent can degrade downstream outputs |

For small local models, start with narrow prompts and expect more manual review.

## Cost, Quota, And Latency

Cortex can call multiple agents during one run. A single workflow may include planning, generation, review, retries, web search context, and final reporting.

Before long runs:

- Confirm which provider is active in `/provider` or the TUI status bar.
- Use smaller prompts for first tests.
- Watch for provider rate-limit errors.
- Prefer local models when privacy or cost is more important than output quality.
- Prefer stronger remote coding models when generated code quality matters more than cost.

Runtime cost tracking is still a product gap. Until it is implemented, treat provider dashboards as the source of truth for billing.

## Privacy Notes

Remote providers may receive:

- The user prompt.
- Agent system prompts.
- Selected project context.
- Web search context when enabled.
- Generated intermediate artifacts needed by downstream agents.

Do not run remote-provider workflows on confidential repositories unless your provider choice and organization policy allow it.

## Troubleshooting Provider Failures

When a run fails, record:

- The command or slash command used.
- Provider and model shown in config or the TUI.
- Whether web search was enabled.
- The first provider error in logs.
- Whether the same prompt works with a smaller scope.

Common symptoms:

- Authentication error: reconnect with `/connect` or reset the API key with `/apikey`.
- Rate limit: retry later, lower parallelism, or switch provider.
- Weak generated output: use a stronger model or reduce project scope.
- Unsupported model behavior: try a mainstream chat or coding model for the same provider.
~~~

- [ ] **Step 2: Verify the provider guide**

Run:

```bash
sed -n '1,260p' docs/PROVIDERS.md
```

Expected: the file prints without shell errors, and all tables are readable.

- [ ] **Step 3: Commit the provider guide**

Run:

```bash
git add docs/PROVIDERS.md
git commit -m "docs: add provider guide"
```

Expected: one commit containing only `docs/PROVIDERS.md`.

---

### Task 3: Add Failed Run Issue Template

**Files:**
- Create: `.github/ISSUE_TEMPLATE/failed_run.md`

- [ ] **Step 1: Create the failed-run template**

Add `.github/ISSUE_TEMPLATE/failed_run.md` with this content:

~~~markdown
---
name: Failed Cortex run
about: Report a workflow run that failed, stalled, or produced unusable output
labels: bug, run-failure
assignees: ''
---

## Summary

What were you trying to generate or review?

## Command

```bash
cortex start "..." --auto
```

Or paste the REPL slash command you used.

## Environment

- Cortex version:
- OS:
- Install method: installer / cargo / release binary
- Workflow: dev / code-review / marketing / prospecting / custom
- Provider:
- Model:
- Web search enabled: yes / no

## Expected result

What did you expect Cortex to create or report?

## Actual result

What happened instead?

## Failure point

- [ ] Provider/auth error
- [ ] Workflow stalled
- [ ] Tool execution failed
- [ ] Build/test failed in generated project
- [ ] Generated files were missing
- [ ] Generated files were low quality or inconsistent
- [ ] TUI/input/resume issue
- [ ] Other

## Logs and artifacts

Paste the smallest useful excerpt. Redact secrets before posting.

Safe to include:

- Error messages.
- Final summary.
- Generated project tree.
- Non-sensitive command output.

Do not include:

- API keys.
- OAuth tokens.
- SMTP credentials.
- Private customer data.
- Proprietary source code unless you are allowed to share it.

## Reproduction steps

1. Configure provider:
2. Run command:
3. Observe:

## Additional context

Any provider limits, unusual project files, custom agents, custom workflows, or resume steps involved?
~~~

- [ ] **Step 2: Verify template frontmatter**

Run:

```bash
sed -n '1,180p' .github/ISSUE_TEMPLATE/failed_run.md
```

Expected: YAML frontmatter is present, closed by `---`, and follows the style of existing templates.

- [ ] **Step 3: Commit the issue template**

Run:

```bash
git add .github/ISSUE_TEMPLATE/failed_run.md
git commit -m "docs: add failed run issue template"
```

Expected: one commit containing only `.github/ISSUE_TEMPLATE/failed_run.md`.

---

### Task 4: Link Beta Resources From README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a beta resources section near the existing beta status**

After the existing status paragraph near the top of `README.md`, add:

```markdown
### Beta resources

- [Beta guide](docs/BETA.md) — recommended workflow, support stance, limits, and good beta prompts.
- [Providers guide](docs/PROVIDERS.md) — provider support levels, model expectations, cost/privacy notes, and troubleshooting.
- [Failed run report](.github/ISSUE_TEMPLATE/failed_run.md) — what to include when a run fails or produces unusable output.
```

- [ ] **Step 2: Add the docs to the table of contents**

In the README table of contents, insert a new item after "Quick Start":

```markdown
4. [Beta Resources](#4-beta-resources)
```

Then increment the following top-level numbers by one so the table of contents remains sequential.

- [ ] **Step 3: Add a matching section after Quick Start**

After the Quick Start section and before Configuration, add:

```markdown
## 4. Beta Resources

Cortex is in beta, so start with the `dev` workflow and a small, concrete prompt before trying broad or custom workflows.

- Read the [Beta guide](docs/BETA.md) for workflow support levels, current limits, and prompt guidance.
- Read the [Providers guide](docs/PROVIDERS.md) before switching models or debugging provider-specific failures.
- Use the [failed run issue template](.github/ISSUE_TEMPLATE/failed_run.md) when a workflow fails, stalls, or produces unusable output.
```

Renumber all subsequent top-level sections by one if the README uses numbered section headings.

- [ ] **Step 4: Verify README links**

Run:

```bash
rg -n "docs/BETA.md|docs/PROVIDERS.md|failed_run.md|Beta Resources" README.md
```

Expected: all three links appear, and the Beta Resources section is present.

- [ ] **Step 5: Commit README links**

Run:

```bash
git add README.md
git commit -m "docs: link beta resources"
```

Expected: one commit containing only `README.md`.

---

### Task 5: Mark Completed Lacunes

**Files:**
- Modify: `LACUNES.md`

- [ ] **Step 1: Add status lines to every lacune**

For each numbered lacune heading in `LACUNES.md`, add a status line immediately after the heading.

For the completed items, use these exact status blocks:

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md`, qui définit le workflow phare, les workflows expérimentaux et les limites beta.
```

Use that block for lacune 4.

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `docs/PROVIDERS.md`, qui documente les niveaux de support, les recommandations modèles et les limites provider.
```

Use that block for lacune 5.

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md` et les liens ajoutés dans `README.md`.
```

Use that block for lacune 10.

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md`, qui choisit `dev` comme chemin beta recommandé et cadre les autres workflows.
```

Use that block for lacune 16.

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `.github/ISSUE_TEMPLATE/failed_run.md`, qui structure les retours de runs échoués.
```

Use that block for lacune 18.

```markdown
**Statut:** Terminé
**Preuve:** Couvert par `docs/BETA.md`, qui recadre la promesse beta et précise les limites du résultat généré.
```

Use that block for lacune 19.

For every other lacune, use:

```markdown
**Statut:** À faire
**Preuve:** Non traité dans ce lot.
```

- [ ] **Step 2: Update recommended next steps**

In `LACUNES.md`, keep the existing recommended next steps but append a short progress note:

```markdown
## Suivi des lots

- 2026-05-18 — Lot docs/process beta terminé: guide beta, guide providers, template failed run, liens README. Lacunes terminées: 4, 5, 10, 16, 18, 19.
```

- [ ] **Step 3: Verify completed items have proof**

Run:

```bash
rg -n "\\*\\*Statut:\\*\\* Terminé|\\*\\*Preuve:\\*\\*" LACUNES.md
```

Expected: every completed status is followed by a concrete proof line.

- [ ] **Step 4: Commit lacune tracking**

Run:

```bash
git add LACUNES.md
git commit -m "docs: track beta readiness lacunes"
```

Expected: one commit containing only `LACUNES.md`.

---

### Task 6: Final Documentation Verification

**Files:**
- Verify: `docs/BETA.md`
- Verify: `docs/PROVIDERS.md`
- Verify: `.github/ISSUE_TEMPLATE/failed_run.md`
- Verify: `README.md`
- Verify: `LACUNES.md`

- [ ] **Step 1: Confirm no Rust files changed**

Run:

```bash
git diff --name-only HEAD~5..HEAD
```

Expected: output includes only Markdown files under `docs/`, `.github/ISSUE_TEMPLATE/`, `README.md`, and `LACUNES.md`.

- [ ] **Step 2: Check repository status**

Run:

```bash
git status --short
```

Expected: only pre-existing unrelated untracked local files remain, such as `.DS_Store`, `.claude/`, or `.idea/`. No intended documentation changes are unstaged.

- [ ] **Step 3: Check link targets exist**

Run:

```bash
test -f docs/BETA.md && test -f docs/PROVIDERS.md && test -f .github/ISSUE_TEMPLATE/failed_run.md
```

Expected: command exits successfully with no output.

- [ ] **Step 4: Review final diff summary**

Run:

```bash
git show --stat --oneline HEAD
```

Expected: latest commit summary is visible and contains only documentation changes.
