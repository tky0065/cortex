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
