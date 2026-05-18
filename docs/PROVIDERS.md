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
