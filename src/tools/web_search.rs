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
        .user_agent("Mozilla/5.0 (compatible; cortex-agent)")
        .build()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let resp = match client
        .get("https://lite.duckduckgo.com/lite/")
        .query(&[("q", query)])
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
    // Extract text snippets — strip HTML tags, collapse whitespace.
    let text = strip_html_tags(&html);
    if text.trim().is_empty() {
        return String::new();
    }
    // Truncate to ~4000 chars to stay within context budget.
    let truncated = if text.len() > 4000 {
        format!("{}…", &text[..4000])
    } else {
        text
    };
    format!(
        "\n\n## Web Search Results (DuckDuckGo)\nQuery: {}\n\n{}\n",
        query, truncated
    )
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse runs of whitespace
    out.split_whitespace().collect::<Vec<_>>().join(" ")
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
    if config.api_keys.web_search.is_none() {
        return String::new();
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
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
