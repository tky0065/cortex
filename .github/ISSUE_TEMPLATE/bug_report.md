---
name: Bug report
about: Something crashed or behaved unexpectedly
labels: bug
assignees: ''
---

## Describe the bug

A clear and concise description of what went wrong.

## Steps to reproduce

```
cortex start "..." --workflow dev
```

1. ...
2. ...

## Expected behavior

What you expected to happen.

## Actual behavior

What actually happened. Paste the error output here.

<details>
<summary>Verbose log (attach if available)</summary>

```
cortex --verbose start "..." 2>&1 | tee cortex.log
```

Paste log here or attach the file.
</details>

## Environment

| Field | Value |
|-------|-------|
| Cortex version | `cortex --version` |
| OS | e.g. macOS 15.2 / Ubuntu 24.04 |
| Rust version | `rustc --version` |
| Provider | e.g. Ollama 0.6, OpenRouter |
| Model | e.g. qwen2.5-coder:32b |
