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
- `cortex.run.json` after reviewing it for private project details.

Do not include:

- API keys.
- OAuth tokens.
- SMTP credentials.
- Private customer data.
- Proprietary source code unless you are allowed to share it.
- Full `cortex.log` output unless you have reviewed and minimized it.

## Reproduction steps

1. Configure provider:
2. Run command:
3. Observe:

## Additional context

Any provider limits, unusual project files, custom agents, custom workflows, or resume steps involved?
