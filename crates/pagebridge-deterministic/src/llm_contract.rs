//! Contract every LLM provider must honour in deterministic mode.
//!
//! Providers self-report their determinism capabilities via
//! [`DeterminismContract`]. The facade refuses to enter deterministic
//! mode if any configured provider returns `supports_seed = false`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterminismContract {
    pub provider: String,
    pub model: String,
    pub supports_seed: bool,
    pub supports_zero_temperature: bool,
    pub supports_top_p_one: bool,
    /// Free-form caveats for the documentation. Examples:
    /// "OpenAI seed is best-effort; identical inputs may differ across
    /// `system_fingerprint` rotations." or "Ollama seed is per-token
    /// only at low temperature."
    pub caveats: Vec<String>,
}

impl DeterminismContract {
    #[must_use]
    pub fn fully_deterministic(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            supports_seed: true,
            supports_zero_temperature: true,
            supports_top_p_one: true,
            caveats: Vec::new(),
        }
    }

    #[must_use]
    pub fn non_deterministic(
        provider: impl Into<String>,
        model: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            supports_seed: false,
            supports_zero_temperature: false,
            supports_top_p_one: false,
            caveats: vec![reason.into()],
        }
    }

    #[must_use]
    pub fn is_strict_safe(&self) -> bool {
        self.supports_seed && self.supports_zero_temperature && self.supports_top_p_one
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_deterministic_is_strict_safe() {
        assert!(DeterminismContract::fully_deterministic("ollama", "qwen2.5").is_strict_safe());
    }

    #[test]
    fn non_deterministic_is_not_strict_safe() {
        assert!(
            !DeterminismContract::non_deterministic("openai", "gpt-4o-rt", "no seed support")
                .is_strict_safe()
        );
    }
}
