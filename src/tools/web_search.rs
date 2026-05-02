#![allow(dead_code)]

use crate::config::Config;
use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Free web search via DuckDuckGo Lite HTML — no API key required.
/// Returns a formatted Markdown block suitable for injection into an LLM prompt.
pub async fn search_without_key(query: &str) -> String {
    if query.trim().is_empty() {
        return String::new();
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let resp = match client
        .post("https://lite.duckduckgo.com/lite/")
        .form(&[("q", query)])
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return String::new(),
    };
    let html = match resp.text().await {
        Ok(t) => t,
        Err(_) => return String::new(),
    };

    let results = parse_ddg_lite_html(&html);
    if results.is_empty() {
        return String::new();
    }

    let mut block = format!(
        "\n\n## Web Search Results (DuckDuckGo Lite)\nQuery: {}\n\n",
        query
    );
    for (i, r) in results.iter().take(5).enumerate() {
        block.push_str(&format!(
            "{}. **{}** ({})\n   {}\n\n",
            i + 1,
            r.title,
            r.url,
            r.snippet
        ));
    }
    block
}

fn parse_ddg_lite_html(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut remaining = html;

    // DuckDuckGo Lite result rows are usually within <table> tags.
    // Each result is typically a series of <tr>s.
    // We'll use a simple but robust string-based extraction for titles, links, and snippets.

    while let Some(start_idx) = remaining.find("class=\"result-link\"") {
        let after_link_class = &remaining[start_idx..];

        // Extract URL
        let url = if let Some(href_start) = after_link_class.find("href=\"") {
            let start = href_start + 6;
            if let Some(href_end) = after_link_class[start..].find("\"") {
                after_link_class[start..start + href_end].to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Extract Title
        let title = if let Some(title_start) = after_link_class.find(">") {
            let start = title_start + 1;
            if let Some(title_end) = after_link_class[start..].find("</a>") {
                strip_html_tags(&after_link_class[start..start + title_end])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Look for the snippet in the following rows
        let mut snippet = String::new();
        if let Some(snippet_idx) = after_link_class.find("class=\"result-snippet\"") {
            let after_snippet_class = &after_link_class[snippet_idx..];
            if let Some(content_start) = after_snippet_class.find(">") {
                let start = content_start + 1;
                if let Some(content_end) = after_snippet_class[start..].find("</td>") {
                    snippet = strip_html_tags(&after_snippet_class[start..start + content_end]);
                }
            }
        }

        if !title.is_empty() && !url.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        // Advance past this result
        if let Some(next_tr) = after_link_class.find("</tr>") {
            remaining = &after_link_class[next_tr + 5..];
        } else {
            break;
        }
    }

    results
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse runs of whitespace and decode basic entities if needed
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Performs a web search. If `WEB_SEARCH_API_KEY` is unset, returns a
/// single stub result so the agent can still run in offline mode.
pub async fn search(query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        bail!("search query must not be empty");
    }

    let api_key = std::env::var("WEB_SEARCH_API_KEY").unwrap_or_default();

    if api_key.is_empty() {
        // Offline stub — real provider wired when WEB_SEARCH_API_KEY is set.
        return Ok(vec![SearchResult {
            title: format!("Search results for: {}", query),
            url: "https://example.com".into(),
            snippet: format!(
                "[offline mode] No WEB_SEARCH_API_KEY set. Query was: {}",
                query
            ),
        }]);
    }

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", query), ("count", &max_results.to_string())])
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Brave Search request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("Brave Search API error: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| anyhow::anyhow!("Brave Search parse failed: {e}"))?;

    let results = resp["web"]["results"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|r| SearchResult {
            title: r["title"].as_str().unwrap_or("").to_string(),
            url: r["url"].as_str().unwrap_or("").to_string(),
            snippet: r["description"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    Ok(results)
}

/// Returns a formatted Markdown block of web search results to inject into an agent prompt.
/// Returns an empty string when web search is disabled, the API key is missing, or search fails.
pub async fn fetch_context(query: &str, config: &Config) -> String {
    if !config.tools.web_search_enabled {
        return String::new();
    }

    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if config.api_keys.web_search.is_none() {
        // Fallback to free search (no key required)
        return search_without_key(trimmed).await;
    }

    match search(trimmed, 5).await {
        Err(_) => String::new(),
        Ok(results) if results.is_empty() => String::new(),
        Ok(results) => {
            // Skip the offline stub result — it adds no value to the LLM prompt.
            if results.len() == 1 && results[0].snippet.contains("[offline mode]") {
                return String::new();
            }
            let mut block = String::from("\n\n## Web Search Results\n");
            for (i, r) in results.iter().enumerate() {
                block.push_str(&format!(
                    "{}. **{}** ({})\n   {}\n",
                    i + 1,
                    r.title,
                    r.url,
                    r.snippet
                ));
            }
            block
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_query_errors() {
        assert!(search("", 5).await.is_err());
    }

    #[tokio::test]
    async fn offline_mode_returns_stub() {
        // No env var set in test environment → stub result
        // SAFETY: single-threaded test, no other thread reads this env var
        unsafe { std::env::remove_var("WEB_SEARCH_API_KEY") };
        let results = search("Rust async traits", 3).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].snippet.contains("offline mode"));
    }

    #[tokio::test]
    async fn fetch_context_disabled_returns_empty() {
        let config = Config::default();
        let ctx = fetch_context("Rust async traits", &config).await;
        assert!(
            ctx.is_empty(),
            "should be empty when web_search_enabled is false"
        );
    }

    #[tokio::test]
    async fn fetch_context_no_key_returns_empty() {
        let mut config = Config::default();
        config.tools.web_search_enabled = true;
        // api_keys.web_search is None by default
        let ctx = fetch_context("Rust async traits", &config).await;
        assert!(ctx.is_empty(), "should be empty when api key is not set");
    }
}
