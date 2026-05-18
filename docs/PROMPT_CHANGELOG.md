# Prompt Changelog

This document tracks significant changes to agent system prompts. Prompt changes can affect workflow quality as much as code changes — treat them with the same care.

## Conventions

### Versioning

Prompts are versioned implicitly through git. Each prompt file includes a `<!-- version: YYYY-MM-DD -->` comment at the top. When you modify a prompt, update this date and add an entry here.

### Review requirements

All prompt changes must:

1. Include a description of what changed and why.
2. Reference any eval scenario affected or a note that no eval exists yet.
3. Be reviewed by at least one team member before merging to `main`.

### Severity levels

| Level | Description | Eval required before merge |
|-------|-------------|---------------------------|
| **major** | Changed agent role, goal, or output format | Yes |
| **minor** | Rephrased instructions, added/removed a section | Recommended |
| **patch** | Typo fix, whitespace, inline comment | No |

---

## Log

### 2026-05-18

**Workflow:** dev  
**Agents affected:** all (ceo, pm, tech_lead, developer, qa, devops)  
**Severity:** minor  
**Change:** Added `## Web Search` section to all 6 dev workflow prompts instructing agents to use injected web search results when available.  
**Eval impact:** No existing eval scenario explicitly tests web search injection. Coverage to be added in `evals/dev/scenarios/`.

---

## Prompt file locations

| Workflow | Agent | File |
|----------|-------|------|
| dev | ceo | `src/workflows/dev/prompts/ceo.md` |
| dev | pm | `src/workflows/dev/prompts/pm.md` |
| dev | tech_lead | `src/workflows/dev/prompts/tech_lead.md` |
| dev | developer | `src/workflows/dev/prompts/developer.md` |
| dev | qa | `src/workflows/dev/prompts/qa.md` |
| dev | devops | `src/workflows/dev/prompts/devops.md` |
| marketing | strategist | `src/workflows/marketing/prompts/strategist.md` |
| marketing | copywriter | `src/workflows/marketing/prompts/copywriter.md` |
| marketing | analyst | `src/workflows/marketing/prompts/analyst.md` |
| marketing | social_media_manager | `src/workflows/marketing/prompts/social_media_manager.md` |
| prospecting | researcher | `src/workflows/prospecting/prompts/researcher.md` |
| prospecting | profiler | `src/workflows/prospecting/prompts/profiler.md` |
| prospecting | copywriter | `src/workflows/prospecting/prompts/copywriter.md` |
| prospecting | outreach_manager | `src/workflows/prospecting/prompts/outreach_manager.md` |

## Adding a new prompt

1. Create the `.md` file under the appropriate `prompts/` directory.
2. Add a `<!-- version: YYYY-MM-DD -->` header comment.
3. Register the role in `src/providers/mod.rs` → `model_for_role()`.
4. Add an entry to this changelog.
5. Add or update an eval scenario in `evals/<workflow>/scenarios/`.
