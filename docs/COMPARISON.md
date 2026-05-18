# Cortex vs. other AI coding tools

Cortex occupies a specific niche that sets it apart from every other AI coding tool.

## The one-line difference

**Cortex generates a complete, deployable Git repository from a single natural-language idea — entirely in your terminal, with no browser, no account, and no cloud workspace required.**

## Comparison matrix

| Capability | Cortex | Claude Code | Cursor | Aider | Copilot Workspace | Devin |
|------------|--------|-------------|--------|-------|-------------------|-------|
| Multi-agent pipeline (CEO→PM→Dev→QA→DevOps) | ✅ | ❌ | ❌ | ❌ | Partial | ✅ |
| Runs fully local (Ollama) | ✅ | ❌ | ❌ | ✅ | ❌ | ❌ |
| Terminal-only, no browser required | ✅ | ✅ | ❌ | ✅ | ❌ | ❌ |
| Custom agents / custom workflows | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Generates complete repo (not patch-level) | ✅ | Partial | ❌ | Partial | Partial | ✅ |
| TUI with live pipeline view | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| No account required | ✅ | ❌ | ❌ | ✅ | ❌ | ❌ |
| Plug-in LLM provider | ✅ | Limited | ❌ | ✅ | ❌ | ❌ |
| Code-review workflow | ✅ | ✅ | ✅ | Partial | ✅ | ✅ |
| Marketing / prospecting workflows | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Resume interrupted runs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |

## When Cortex is the right tool

- **Greenfield project generation**: you have an idea and want a working, structured repo in minutes — not just a file or a patch.
- **Local-first / air-gapped**: you need to run everything on your own hardware without sending code to a cloud service.
- **Custom multi-agent pipelines**: you want to define your own roles, prompts, and workflow steps for domain-specific generation.
- **Non-dev workflows**: marketing briefs, prospecting outreach, and custom knowledge-work pipelines that other coding tools don't support.

## When Cortex is not the right tool

- **Editing an existing large codebase interactively**: tools like Cursor, Claude Code, or Aider are better suited for in-context line-by-line editing.
- **Real-time pair programming**: Cortex runs workflows end-to-end; it is not a chat-driven copilot for incremental changes.
- **Enterprise cloud-managed environments**: Cortex is a local CLI, not a SaaS platform.

## Beta focus

During beta, Cortex's primary validated use case is the **`dev` workflow**: generating a complete, buildable software project from a one-line idea. Other workflows are available but considered experimental until explicitly validated.
