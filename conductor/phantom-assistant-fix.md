# Plan: Fix phantom "assistant" panel and tool call parsing

## Objective
Address the visual bug where a phantom "assistant" panel appears alongside "cortex", and fix the XML parsing issue where non-standard `<tool_call>` formats (e.g., `<web_search>{json}</web_search>`) fail to parse and are displayed as raw XML.

## Key Files & Context
- `src/assistant.rs`: Contains the hardcoded `"assistant"` string for the UI, `parse_tool_calls`, `parse_single_call`, and `strip_tool_calls_for_display`.
- `src/tools/web_search.rs`: Needs a DuckDuckGo Lite fallback when the Brave API key is missing.
- `src/tui/events.rs` and `src/repl.rs`: Event handling for the TUI labels.
- `src/tui/mod.rs` and `src/tui/widgets/input.rs`: TUI component labels.

## Implementation Steps
1. **Fix Phantom Panel**:
   - In `src/assistant.rs`, change the hardcoded string `"assistant"` to `"cortex"` in `complete_chat_stream` (around line 1098).
2. **Improve XML Parsing (`parse_single_call`)**:
   - Update `parse_single_call` to fallback to detecting `<tool_name>{json}</tool_name>` if the standard `<name>...</name>` format is not found. This handles the model NEMOTRON-3-NANO-4B's output format.
3. **Enhance XML Stripping (`strip_tool_calls_for_display`)**:
   - Update `strip_tool_calls_for_display` to also remove known `<tool_name>` tags (e.g. `<web_search>`) and orphan `</tool_call>` tags to prevent raw XML from being displayed.
4. **Generalist Agent Updates**:
   - Rewrite PREAMBLE in `src/assistant.rs` to support the new generalist open-domain identity, making `web_search` mandatory for unknown subjects.
5. **Web Search Fallback**:
   - Add a DDG Lite fallback (`search_without_key`) in `src/tools/web_search.rs`.
   - Modify the WebSearch tool handler in `src/assistant.rs` to use this fallback if no Brave key is present.
6. **Rename 'assistant' to 'cortex' Across TUI**:
   - Update `TuiEvent`s and label strings in `src/repl.rs`, `src/tui/mod.rs`, and `src/tui/widgets/input.rs`.

## Verification & Testing
- Run `cargo check`, `cargo clippy`, and `cargo test` to ensure all changes are sound.