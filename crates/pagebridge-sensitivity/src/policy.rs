use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::label::SensitivityLabel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllowSet {
    /// Maximum tier the caller may access. Defaults to 0 (Public-only).
    pub max_tier: u8,
    /// Explicit category allowlist. If non-empty, PII categories not in
    /// this set are denied even when tier permits.
    pub pii_categories: HashSet<String>,
    /// Allow PHI regardless of tier (typically requires BAA).
    pub allow_phi: bool,
}

pub struct AccessPolicy {
    pub allow: AllowSet,
}

impl AccessPolicy {
    #[must_use]
    pub fn least_privilege() -> Self {
        Self {
            allow: AllowSet {
                max_tier: 0,
                pii_categories: HashSet::new(),
                allow_phi: false,
            },
        }
    }

    #[must_use]
    pub fn with_tier(mut self, tier: u8) -> Self {
        self.allow.max_tier = tier;
        self
    }

    #[must_use]
    pub fn with_phi(mut self) -> Self {
        self.allow.allow_phi = true;
        self
    }

    #[must_use]
    pub fn with_pii_categories<I: IntoIterator<Item = String>>(mut self, cats: I) -> Self {
        self.allow.pii_categories.extend(cats);
        self
    }

    /// Decide if the caller is permitted to read a node carrying `label`.
    #[must_use]
    pub fn permits(&self, label: &SensitivityLabel) -> bool {
        match label {
            SensitivityLabel::Phi => self.allow.allow_phi,
            SensitivityLabel::Pii { categories } => {
                if label.tier() > self.allow.max_tier {
                    return false;
                }
                if self.allow.pii_categories.is_empty() {
                    return false;
                }
                categories
                    .iter()
                    .all(|c| self.allow.pii_categories.contains(c))
            }
            _ => label.tier() <= self.allow.max_tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_only_permits_public() {
        let p = AccessPolicy::least_privilege();
        assert!(p.permits(&SensitivityLabel::Public));
        assert!(!p.permits(&SensitivityLabel::Internal));
        assert!(!p.permits(&SensitivityLabel::Phi));
    }

    #[test]
    fn tier_3_admits_restricted_but_rejects_phi() {
        let p = AccessPolicy::least_privilege().with_tier(3);
        assert!(p.permits(&SensitivityLabel::Restricted));
        assert!(!p.permits(&SensitivityLabel::Phi));
    }

    #[test]
    fn pii_requires_category_allowlist() {
        let p = AccessPolicy::least_privilege()
            .with_tier(3)
            .with_pii_categories(["email".to_string(), "name".to_string()]);
        let allowed = SensitivityLabel::Pii {
            categories: vec!["email".into(), "name".into()],
        };
        let denied = SensitivityLabel::Pii {
            categories: vec!["ssn".into()],
        };
        assert!(p.permits(&allowed));
        assert!(!p.permits(&denied));
    }
}
