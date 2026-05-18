---
name: Security report
about: Report a security vulnerability, secret exposure, or unsafe behavior
labels: security
assignees: ''
---

<!-- IMPORTANT: For critical vulnerabilities (RCE, secret exfiltration, supply chain), consider using GitHub's private Security Advisory feature instead of a public issue. -->

## Summary

A one-sentence description of the security issue.

## Category

- [ ] Prompt injection (LLM output used to construct dangerous commands or paths)
- [ ] Path traversal (file access outside the project output directory)
- [ ] Secret exposure (API key, token, or credential leaked in logs, outputs, or generated files)
- [ ] Unsafe command execution (terminal tool bypass or non-allowlisted command)
- [ ] Supply chain (dependency vulnerability, binary tampering)
- [ ] Other

## Environment

- Cortex version: (`cortex --version`)
- OS:
- Install method: installer / cargo / release binary
- Provider used:

## Steps to reproduce

Describe how to trigger the issue. Include the minimum input needed:

1. Configure / install:
2. Run command:
3. Observe:

## Impact

What can an attacker do? What data is exposed or what action can be triggered?

## Suggested fix

If you have an idea for a fix, describe it here. Otherwise leave blank.

## Evidence

Paste logs, generated file snippets, or tool outputs that demonstrate the issue. **Redact any real secrets before posting.**
