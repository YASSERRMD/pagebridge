//! Capability vocabulary for pagebridge tokens.
//!
//! Capabilities are short strings that gate operations. A token carrying
//! `ask` may call `pagebridge.ask` and `pagebridge.search`; a token carrying
//! `ingest` may upload documents. The vocabulary is intentionally tiny so
//! tokens stay readable when minted by hand.

use std::collections::HashSet;
use std::str::FromStr;

use pagebridge_core::error::{PagebridgeError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read documents, search, navigate, ask questions.
    Read,
    /// Run cited `ask` queries (implies `Read`).
    Ask,
    /// Ingest documents.
    Ingest,
    /// Remove documents and reset state.
    Admin,
}

impl Capability {
    /// Stable wire name used in Biscuit facts and CLI input.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Ask => "ask",
            Self::Ingest => "ingest",
            Self::Admin => "admin",
        }
    }

    /// True if this capability implies `other`. `admin` implies everything,
    /// `ingest` implies `read`, `ask` implies `read`.
    #[must_use]
    pub fn implies(self, other: Self) -> bool {
        if self == other {
            return true;
        }
        match self {
            Self::Admin => true,
            Self::Ingest | Self::Ask => matches!(other, Self::Read),
            Self::Read => false,
        }
    }
}

impl FromStr for Capability {
    type Err = PagebridgeError;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "ask" => Ok(Self::Ask),
            "ingest" => Ok(Self::Ingest),
            "admin" => Ok(Self::Admin),
            other => Err(PagebridgeError::InvalidArgument(format!(
                "unknown capability: {other}"
            ))),
        }
    }
}

/// Set of capabilities a token carries.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    inner: HashSet<Capability>,
}

impl CapabilitySet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, cap: Capability) {
        self.inner.insert(cap);
    }

    /// True if any held capability implies `needed`.
    #[must_use]
    pub fn permits(&self, needed: Capability) -> bool {
        self.inner.iter().any(|c| c.implies(needed))
    }

    /// Iterate held capabilities in stable order (read < ask < ingest < admin).
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        let mut all: Vec<_> = self.inner.iter().copied().collect();
        all.sort_by_key(|c| match c {
            Capability::Read => 0,
            Capability::Ask => 1,
            Capability::Ingest => 2,
            Capability::Admin => 3,
        });
        all.into_iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(it: I) -> Self {
        Self {
            inner: it.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_implies_everything() {
        for cap in [
            Capability::Read,
            Capability::Ask,
            Capability::Ingest,
            Capability::Admin,
        ] {
            assert!(Capability::Admin.implies(cap));
        }
    }

    #[test]
    fn ask_implies_read_but_not_ingest() {
        assert!(Capability::Ask.implies(Capability::Read));
        assert!(!Capability::Ask.implies(Capability::Ingest));
    }

    #[test]
    fn capability_set_permits_correctly() {
        let set: CapabilitySet = [Capability::Ask].into_iter().collect();
        assert!(set.permits(Capability::Read));
        assert!(set.permits(Capability::Ask));
        assert!(!set.permits(Capability::Ingest));
    }

    #[test]
    fn capability_parses_from_lower_and_mixed_case() {
        assert_eq!(Capability::from_str("ASK").unwrap(), Capability::Ask);
        assert_eq!(
            Capability::from_str(" ingest ").unwrap(),
            Capability::Ingest
        );
        assert!(Capability::from_str("nuke").is_err());
    }
}
