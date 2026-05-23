//! Biscuit-backed capability tokens.
//!
//! Layout of a minted Biscuit:
//! - Authority block contains `capability("ask")`, `capability("ingest")`,
//!   etc., plus optional `workspace("acme")` and `expires_at(<unix>)` facts.
//! - Verification runs a small Datalog policy that checks expiry and reads
//!   back the capability and workspace facts.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use biscuit_auth::macros::{authorizer, biscuit, fact};
use biscuit_auth::{Biscuit, KeyPair, PublicKey};

use pagebridge_core::error::{PagebridgeError, Result};

use crate::capability::{Capability, CapabilitySet};

/// Information minted into a Biscuit token.
#[derive(Debug, Clone, Default)]
pub struct TokenSpec {
    pub capabilities: CapabilitySet,
    pub workspace: Option<String>,
    pub ttl: Option<Duration>,
    pub note: Option<String>,
}

/// Result of a successful verification.
#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub capabilities: CapabilitySet,
    pub workspace: Option<String>,
    pub expires_at: Option<u64>,
}

/// Mint a base64-url-safe Biscuit token signed by `root`.
pub fn mint(root: &KeyPair, spec: &TokenSpec) -> Result<String> {
    let mut builder = biscuit!("");
    for cap in spec.capabilities.iter() {
        let name = cap.as_str().to_owned();
        builder = builder.fact(fact!("capability({name});")).map_err(err)?;
    }
    if let Some(ws) = &spec.workspace {
        let w = ws.clone();
        builder = builder.fact(fact!("workspace({w});")).map_err(err)?;
    }
    if let Some(ttl) = spec.ttl {
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_add(ttl.as_secs());
        let exp_i: i64 = i64::try_from(exp).unwrap_or(i64::MAX);
        builder = builder
            .fact(fact!("expires_at({exp_i});"))
            .map_err(err)?;
    }
    if let Some(note) = &spec.note {
        let n = note.clone();
        builder = builder.fact(fact!("note({n});")).map_err(err)?;
    }
    let token = builder.build(root).map_err(err)?;
    token.to_base64().map_err(err)
}

/// Verify a token against the root public key. Returns the decoded
/// capability set + metadata, or an error if the token is malformed, expired,
/// or signed by a different key.
pub fn verify(token_str: &str, public: &PublicKey) -> Result<VerifiedToken> {
    let parsed = Biscuit::from_base64(token_str, *public).map_err(err)?;
    let now: i64 = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(i64::MAX);
    let mut authz = authorizer!(
        r#"
        time({now});
        check if expires_at($e), $e >= {now};
        allow if true;
        "#
    )
    .build(&parsed)
    .map_err(err)?;
    authz.authorize().map_err(err)?;

    let caps_query: Vec<(String,)> = authz
        .query("data($name) <- capability($name)")
        .map_err(err)?;
    let ws_query: Vec<(String,)> = authz
        .query("data($name) <- workspace($name)")
        .map_err(err)?;
    let exp_query: Vec<(i64,)> = authz
        .query("data($t) <- expires_at($t)")
        .map_err(err)?;

    let mut set = CapabilitySet::new();
    for (name,) in caps_query {
        if let Ok(cap) = name.parse::<Capability>() {
            set.insert(cap);
        }
    }
    Ok(VerifiedToken {
        capabilities: set,
        workspace: ws_query.into_iter().next().map(|(w,)| w),
        expires_at: exp_query
            .into_iter()
            .next()
            .map(|(t,)| u64::try_from(t).unwrap_or(0)),
    })
}

fn err<E: std::fmt::Display>(e: E) -> PagebridgeError {
    PagebridgeError::Internal(format!("auth: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::RootKey;

    #[test]
    fn mint_and_verify_token() {
        let root = RootKey::generate();
        let mut caps = CapabilitySet::new();
        caps.insert(Capability::Ask);
        caps.insert(Capability::Read);
        let spec = TokenSpec {
            capabilities: caps,
            workspace: Some("acme".into()),
            ttl: Some(Duration::from_secs(60)),
            note: None,
        };
        let token = mint(&root.keypair, &spec).expect("mint");
        let v = verify(&token, &root.keypair.public()).expect("verify");
        assert!(v.capabilities.permits(Capability::Ask));
        assert!(v.capabilities.permits(Capability::Read));
        assert_eq!(v.workspace.as_deref(), Some("acme"));
        assert!(v.expires_at.is_some());
    }

    #[test]
    fn expired_token_is_rejected() {
        let root = RootKey::generate();
        let spec = TokenSpec {
            capabilities: [Capability::Read].into_iter().collect(),
            workspace: None,
            ttl: Some(Duration::from_secs(0)),
            note: None,
        };
        let token = mint(&root.keypair, &spec).expect("mint");
        std::thread::sleep(Duration::from_millis(1100));
        let res = verify(&token, &root.keypair.public());
        assert!(res.is_err(), "expired token should fail");
    }

    #[test]
    fn token_from_wrong_key_is_rejected() {
        let a = RootKey::generate();
        let b = RootKey::generate();
        let spec = TokenSpec {
            capabilities: [Capability::Read].into_iter().collect(),
            ..Default::default()
        };
        let token = mint(&a.keypair, &spec).expect("mint");
        assert!(verify(&token, &b.keypair.public()).is_err());
    }
}
