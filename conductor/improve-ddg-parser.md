# Plan: Improve DuckDuckGo Lite Search Parsing

## Objective
Enhance the fallback web search (`search_without_key`) to properly parse the HTML from DuckDuckGo Lite instead of just stripping all tags. This will provide the agent with structured, multi-source results (titles, URLs, snippets) similar to the Brave Search API.

## Key Files & Context
- `src/tools/web_search.rs`: Contains the `search_without_key` function and HTML stripping logic.

## Implementation Steps
1. **Improve HTML Parsing**:
   - Rewrite the fallback DuckDuckGo parser to look for result rows in the HTML (e.g., matching the `result-snippet`, `result-url`, and `result-title` classes or structures).
   - Alternatively, since writing a full HTML parser using regex or basic string operations can be brittle, we can extract lines that contain "http", and lines that look like snippets.
   - DuckDuckGo Lite structure usually has `<tr>` rows where the title is an `<a>` tag, followed by a snippet in a `<td>` with class `result-snippet`.
   - We will implement a lightweight parser in `search_without_key` that returns a formatted Markdown string containing multiple extracted results.
   
2. **Update `search_without_key`**:
   - Instead of a single blob of stripped text, extract the top 3 to 5 results and format them as:
     `1. **Title** (URL)`
     `   Snippet`

## Verification & Testing
- Run `cargo check` and `cargo test`.
- Optionally, run the agent to verify that it now receives and displays multi-source, structured information when falling back to DuckDuckGo Lite.