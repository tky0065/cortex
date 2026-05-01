# Contributing to Cortex

Thanks for your interest in contributing! Cortex is a solo-maintained project, but external contributions are welcome — please read this guide before opening a PR.

## How to contribute

### Report a bug
Open an issue using the **Bug report** template. Include your OS, Cortex version (`cortex --version`), and the exact command that failed. Attach verbose logs if possible (`cortex --verbose`).

### Request a feature
Open an issue using the **Feature request** template. Describe the use-case first, not the solution — it helps prioritize correctly.

### Submit a PR
1. Open or comment on an issue first so we can agree on the approach before you write code.
2. Fork the repo and create a branch from `main`:
   ```
   git checkout -b feat/my-feature
   ```
3. Follow the code style (run `cargo fmt` and `cargo clippy -- -D warnings` before pushing).
4. Add or update tests where relevant (`cargo test`).
5. Open a PR against `main` with a clear description of what changes and why.

PRs that touch core orchestration, providers, or the TUI without a prior issue discussion may be closed to keep scope manageable.

## Development setup

```bash
git clone https://github.com/your-username/cortex
cd cortex
cargo build
cargo test
cargo run -- --help
```

Requires Rust stable (1.80+). An Ollama instance running locally is needed for end-to-end workflow tests.

## Code conventions

- `cargo fmt` — always run before committing
- `cargo clippy -- -D warnings` — no warnings allowed
- No `unwrap()` in production paths — use `?` or explicit error handling
- New agents go in `src/workflows/<workflow>/agents/` and must include a `.md` prompt file
- Context window safety: never pass the full transcript to downstream agents (file-as-memory pattern)

## Commit style

```
feat(tui): add scrollable diff viewer
fix(provider): handle Ollama timeout gracefully
docs: update provider setup instructions
```

Use `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.

## Branch policy

`main` is protected — direct pushes are disabled. All changes go through PRs, even from the maintainer.

## License

By contributing you agree that your code will be released under the [MIT License](../LICENSE).
