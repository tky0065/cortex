#![allow(dead_code)]

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url:   String,
    pub snippet: String,
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
            title:   format!("Search results for: {}", query),
            url:     "https://example.com".into(),
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
            title:   r["title"].as_str().unwrap_or("").to_string(),
            url:     r["url"].as_str().unwrap_or("").to_string(),
            snippet: r["description"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    Ok(results)
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
}
