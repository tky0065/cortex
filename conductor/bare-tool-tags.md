# Plan: Fix Bare Tool Tag Parsing

## Objective
Update the XML tool parser to properly detect and execute tools when the model (e.g. Nemotron) emits bare tool tags like `<web_search>...</web_search>` without wrapping them in the standard `<tool_call>` block.

## Key Files & Context
- `src/assistant.rs`: Contains the `parse_tool_calls` function which currently only scans for `<tool_call>`.

## Implementation Steps
1. **Update `parse_tool_calls`**:
   - Add a fallback mechanism at the end of the function.
   - If no standard `<tool_call>` blocks are found, pass the entire text to `parse_single_call(text)`.
   - `parse_single_call` already has logic to scan for known tool tags (like `<web_search>`), so it will successfully extract the JSON and return the `ToolCall`.

## Verification & Testing
- Add a new unit test in `src/assistant.rs` to verify that a bare `<web_search>{"query":"enokdev"}</web_search>` is successfully parsed.
- Run `cargo test assistant::tests` to confirm everything works as expected.