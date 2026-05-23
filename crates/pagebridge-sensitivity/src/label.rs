use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SensitivityLabel {
    Public,
    Internal,
    Confidential,
    Restricted,
    Pii { categories: Vec<String> },
    Phi,
    Custom { id: String, tier: u8 },
}

impl SensitivityLabel {
    /// Lower numbers are less sensitive. Used by AccessPolicy to compare
    /// against the caller's allowed maximum tier.
    #[must_use]
    pub fn tier(&self) -> u8 {
        match self {
            Self::Public => 0,
            Self::Internal => 1,
            Self::Confidential => 2,
            Self::Restricted => 3,
            Self::Pii { .. } => 3,
            Self::Phi => 4,
            Self::Custom { tier, .. } => *tier,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
            Self::Restricted => "restricted",
            Self::Pii { .. } => "pii",
            Self::Phi => "phi",
            Self::Custom { .. } => "custom",
        }
    }
}

impl Default for SensitivityLabel {
    fn default() -> Self {
        Self::Public
    }
}
