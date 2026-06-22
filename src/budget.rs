use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    NotApplicable,
    Unknown,
    WithinBudget,
    Exceeded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetLimits {
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetSnapshot {
    pub tokens_total: Option<u64>,
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
    pub estimated_cost_usd: Option<f64>,
    pub status: BudgetStatus,
    pub exceeded_reason: Option<String>,
    pub cost_notes: String,
}

#[derive(Debug, Clone)]
pub struct BudgetState {
    provider: String,
    model: String,
    limits: BudgetLimits,
    tokens_total: Option<u64>,
}

impl BudgetState {
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        limits: BudgetLimits,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            limits,
            tokens_total: None,
        }
    }

    pub fn record_tokens_total(&mut self, tokens_total: u64) {
        self.tokens_total = Some(tokens_total);
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let estimated_cost_usd = self
            .tokens_total
            .and_then(|tokens| estimate_cost_usd(&self.provider, &self.model, tokens));

        if let Some(tokens) = self.tokens_total {
            if self.limits.max_tokens_per_run > 0 && tokens > self.limits.max_tokens_per_run {
                return BudgetSnapshot {
                    tokens_total: self.tokens_total,
                    max_tokens_per_run: self.limits.max_tokens_per_run,
                    max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                    estimated_cost_usd,
                    status: BudgetStatus::Exceeded,
                    exceeded_reason: Some(format!(
                        "token budget exceeded: {} > {}",
                        tokens, self.limits.max_tokens_per_run
                    )),
                    cost_notes: self.cost_notes(estimated_cost_usd),
                };
            }
        }

        if let Some(cost) = estimated_cost_usd {
            if self.limits.max_estimated_cost_usd > 0.0 && cost > self.limits.max_estimated_cost_usd
            {
                return BudgetSnapshot {
                    tokens_total: self.tokens_total,
                    max_tokens_per_run: self.limits.max_tokens_per_run,
                    max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                    estimated_cost_usd,
                    status: BudgetStatus::Exceeded,
                    exceeded_reason: Some("estimated cost budget exceeded".to_string()),
                    cost_notes: self.cost_notes(estimated_cost_usd),
                };
            }

            return BudgetSnapshot {
                tokens_total: self.tokens_total,
                max_tokens_per_run: self.limits.max_tokens_per_run,
                max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                estimated_cost_usd,
                status: BudgetStatus::WithinBudget,
                exceeded_reason: None,
                cost_notes: self.cost_notes(estimated_cost_usd),
            };
        }

        let status = if is_local_provider(&self.provider) {
            BudgetStatus::NotApplicable
        } else {
            BudgetStatus::Unknown
        };

        BudgetSnapshot {
            tokens_total: self.tokens_total,
            max_tokens_per_run: self.limits.max_tokens_per_run,
            max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
            estimated_cost_usd,
            status,
            exceeded_reason: None,
            cost_notes: self.cost_notes(estimated_cost_usd),
        }
    }

    fn cost_notes(&self, estimated_cost_usd: Option<f64>) -> String {
        if estimated_cost_usd.is_some() {
            return "Estimated from local static provider/model pricing; actual billing may differ."
                .to_string();
        }
        if is_local_provider(&self.provider) {
            return "Local provider cost is not applicable; token budget can still be enforced when token totals are available.".to_string();
        }
        format!(
            "No local price entry for provider '{}' and model '{}'; cost budget could not be evaluated.",
            self.provider, self.model
        )
    }
}

fn is_local_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "ollama" | "lmstudio" | "local"
    )
}

fn estimate_cost_usd(provider: &str, model: &str, tokens_total: u64) -> Option<f64> {
    let provider = provider.trim().to_ascii_lowercase();
    let model = model.trim().to_ascii_lowercase();
    let usd_per_million_tokens = match (provider.as_str(), model.as_str()) {
        ("openai", "gpt-4.1") | ("openai_chatgpt", "gpt-4.1") => 3.0,
        ("openai", "gpt-4.1-mini") | ("openai_chatgpt", "gpt-4.1-mini") => 0.8,
        ("openrouter", model) if model.contains("openai/gpt-4.1") => 3.0,
        ("groq", model) if model.contains("llama") => 0.6,
        ("together", model) if model.contains("qwen") => 1.2,
        _ => return None,
    };

    Some((tokens_total as f64 / 1_000_000.0) * usd_per_million_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_provider_is_not_applicable_for_cost_until_tokens_arrive() {
        let state = BudgetState::new(
            "ollama",
            "qwen2.5-coder:32b",
            BudgetLimits {
                max_tokens_per_run: 100_000,
                max_estimated_cost_usd: 5.0,
            },
        );

        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::NotApplicable);
        assert_eq!(snapshot.estimated_cost_usd, None);
        assert_eq!(snapshot.exceeded_reason, None);
    }

    #[test]
    fn token_limit_exceeded_when_known_total_is_above_limit() {
        let mut state = BudgetState::new(
            "ollama",
            "qwen2.5-coder:32b",
            BudgetLimits {
                max_tokens_per_run: 10,
                max_estimated_cost_usd: 0.0,
            },
        );

        state.record_tokens_total(11);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Exceeded);
        assert_eq!(
            snapshot.exceeded_reason.as_deref(),
            Some("token budget exceeded: 11 > 10")
        );
    }

    #[test]
    fn zero_limits_disable_enforcement() {
        let mut state = BudgetState::new(
            "openai",
            "gpt-4.1",
            BudgetLimits {
                max_tokens_per_run: 0,
                max_estimated_cost_usd: 0.0,
            },
        );

        state.record_tokens_total(1_000_000);
        let snapshot = state.snapshot();

        assert_ne!(snapshot.status, BudgetStatus::Exceeded);
        assert!(snapshot.exceeded_reason.is_none());
    }

    #[test]
    fn known_openai_model_estimates_cost_and_can_exceed_limit() {
        let mut state = BudgetState::new(
            "openai",
            "gpt-4.1",
            BudgetLimits {
                max_tokens_per_run: 0,
                max_estimated_cost_usd: 0.0001,
            },
        );

        state.record_tokens_total(10_000);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Exceeded);
        assert!(snapshot.estimated_cost_usd.unwrap() > 0.0001);
        assert_eq!(
            snapshot.exceeded_reason.as_deref(),
            Some("estimated cost budget exceeded")
        );
    }

    #[test]
    fn unknown_remote_provider_reports_unknown_cost_without_blocking() {
        let mut state = BudgetState::new(
            "custom_llm",
            "my-model",
            BudgetLimits {
                max_tokens_per_run: 100_000,
                max_estimated_cost_usd: 5.0,
            },
        );

        state.record_tokens_total(1000);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Unknown);
        assert_eq!(snapshot.estimated_cost_usd, None);
        assert!(snapshot.cost_notes.contains("No local price entry"));
    }
}
