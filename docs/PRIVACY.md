# Data & Privacy

Cortex is a local-first CLI. No telemetry, no analytics, no account required.

## What data leaves your machine

Cortex sends data to external services **only** when you explicitly configure a remote provider or tool.

### LLM providers

Every agent call sends:

- The system prompt for that agent role (included in the binary).
- The task prompt derived from your idea or from files written to disk (`specs.md`, `architecture.md`).
- Any web search results injected into the prompt (if web search is enabled).

**What is not sent:** your full shell history, environment variables, files outside the project output directory, or any file you did not explicitly ask Cortex to process.

Providers supported, with their privacy policies:

| Provider | Endpoint | Policy link |
|----------|----------|-------------|
| Ollama (local) | `http://localhost:11434` | No data leaves your machine |
| OpenRouter | `https://openrouter.ai/api/v1` | <https://openrouter.ai/privacy> |
| Groq | `https://api.groq.com` | <https://groq.com/privacy-policy/> |
| Together AI | `https://api.together.xyz` | <https://www.together.ai/privacy> |

If you use a remote provider, the prompt content (your idea + generated documents) is sent to their API. Review each provider's data retention and training policies before sending sensitive information.

### Web search (Brave Search)

When `tools.web_search_enabled = true`, the first ~200 characters of each agent prompt are sent to the Brave Search API as a search query.

- Brave Search API policy: <https://brave.com/privacy/search/>
- You can disable web search at any time: `/websearch disable` in the REPL or set `web_search_enabled = false` in `~/.cortex/config.toml`.

### Email tool (dry-run by default)

The `tools/email.rs` SMTP tool does **not** send emails unless you pass `--send` explicitly. In dry-run mode, the composed message is written to disk only.

## Local logs

When `--verbose` is active, Cortex writes a `cortex.log` file in the working directory. This file may contain:

- Prompt text sent to providers.
- Full LLM responses.
- Tool call inputs and outputs.

**Do not share `cortex.log` publicly** without first reviewing it for secrets, credentials, or private project content.

## API keys and secrets

API keys are stored in `~/.cortex/config.toml` (user home directory, mode 0600 on Unix). They are:

- Never written to project output directories.
- Never included in generated `cortex.manifest.json` (hashes only, no values).
- Redacted from log output where Cortex controls the formatting.

If you discover a secret in generated project files, please open a [security report](https://github.com/tky0065/cortex/issues/new?template=security_report.md).

## Opt-out

Cortex collects no telemetry. There is nothing to opt out of. If you use a remote provider, opt out through that provider's dashboard.

## Questions

Open an issue or see [BETA.md](BETA.md) for support channels.
