//! Capability-based authentication for pagebridge admin and MCP servers.
//!
//! Built on Biscuit tokens (chosen over JWT because Biscuit supports
//! attenuated tokens, which fit the cognitive-agent delegation model). Library
//! users without admin/MCP servers see no behavior change: capabilities are
//! enforced at the server boundary, not inside the facade.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::iter_on_single_items,
    clippy::single_element_loop
)]

pub mod capability;
pub mod keys;
pub mod token;

pub use capability::{Capability, CapabilitySet};
pub use keys::RootKey;
pub use token::{mint, verify, TokenSpec, VerifiedToken};

use std::sync::Arc;

use biscuit_auth::PublicKey;
use pagebridge_core::error::{PagebridgeError, Result};
use parking_lot::RwLock;

/// Server-side verifier: holds the root public key and enforces capability
/// checks on incoming requests.
#[derive(Clone)]
pub struct Authorizer {
    inner: Arc<Inner>,
}

struct Inner {
    public: RwLock<PublicKey>,
    anonymous: CapabilitySet,
}

impl Authorizer {
    /// Build a new authorizer with the given root public key. Anonymous
    /// callers (no `Authorization` header) get `anonymous` capabilities.
    #[must_use]
    pub fn new(public: PublicKey, anonymous: CapabilitySet) -> Self {
        Self {
            inner: Arc::new(Inner {
                public: RwLock::new(public),
                anonymous,
            }),
        }
    }

    /// Rotate the trusted public key in place.
    pub fn set_public_key(&self, public: PublicKey) {
        *self.inner.public.write() = public;
    }

    /// Verify a bearer token, returning the capabilities it carries.
    pub fn verify_bearer(&self, bearer: &str) -> Result<VerifiedToken> {
        let pubkey = *self.inner.public.read();
        verify(bearer, &pubkey)
    }

    /// Top-level guard: parse the optional `Authorization: Bearer ...` value,
    /// check the requested capability, return the verified token if any.
    pub fn check(&self, header: Option<&str>, needed: Capability) -> Result<Option<VerifiedToken>> {
        let Some(value) = header else {
            return if self.inner.anonymous.permits(needed) {
                Ok(None)
            } else {
                Err(PagebridgeError::InvalidArgument(format!(
                    "anonymous calls require capability {:?}; not granted",
                    needed.as_str()
                )))
            };
        };
        let token = strip_bearer(value)?;
        let verified = self.verify_bearer(token)?;
        if verified.capabilities.permits(needed) {
            Ok(Some(verified))
        } else {
            Err(PagebridgeError::InvalidArgument(format!(
                "token lacks capability {:?}",
                needed.as_str()
            )))
        }
    }
}

fn strip_bearer(header: &str) -> Result<&str> {
    let trimmed = header.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .map(str::trim)
        .ok_or_else(|| {
            PagebridgeError::InvalidArgument(
                "expected 'Bearer <token>' authorization header".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn anonymous_calls_with_default_caps_pass() {
        let root = RootKey::generate();
        let anon: CapabilitySet = [Capability::Read].into_iter().collect();
        let authz = Authorizer::new(root.keypair.public(), anon);
        assert!(authz.check(None, Capability::Read).unwrap().is_none());
        assert!(authz.check(None, Capability::Ingest).is_err());
    }

    #[test]
    fn bearer_token_unlocks_capability() {
        let root = RootKey::generate();
        let authz = Authorizer::new(root.keypair.public(), CapabilitySet::new());
        let token = mint(
            &root.keypair,
            &TokenSpec {
                capabilities: [Capability::Ingest].into_iter().collect(),
                ttl: Some(Duration::from_secs(60)),
                ..Default::default()
            },
        )
        .unwrap();
        let auth_header = format!("Bearer {token}");
        let v = authz
            .check(Some(&auth_header), Capability::Ingest)
            .unwrap()
            .unwrap();
        assert!(v.capabilities.permits(Capability::Read));
    }

    #[test]
    fn bearer_lacking_capability_is_rejected() {
        let root = RootKey::generate();
        let authz = Authorizer::new(root.keypair.public(), CapabilitySet::new());
        let token = mint(
            &root.keypair,
            &TokenSpec {
                capabilities: [Capability::Read].into_iter().collect(),
                ttl: Some(Duration::from_secs(60)),
                ..Default::default()
            },
        )
        .unwrap();
        let auth_header = format!("Bearer {token}");
        assert!(authz.check(Some(&auth_header), Capability::Ingest).is_err());
    }
}
