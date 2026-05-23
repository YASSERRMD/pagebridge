//! Root key management for pagebridge capability tokens.
//!
//! The root key signs every minted Biscuit token. The private half is stored
//! in a file (default `~/.pagebridge/root.key`) and held only in memory long
//! enough to sign or verify. `zeroize` clears the key material on drop.

use std::fs;
use std::path::Path;

use biscuit_auth::{KeyPair, PrivateKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use pagebridge_core::error::{PagebridgeError, Result};

/// Owned root keypair plus the hex-encoded public key for distribution.
pub struct RootKey {
    pub keypair: KeyPair,
    pub public_hex: String,
}

#[derive(Serialize, Deserialize)]
struct StoredKey {
    private_hex: String,
}

impl Drop for StoredKey {
    fn drop(&mut self) {
        self.private_hex.zeroize();
    }
}

impl RootKey {
    /// Generate a fresh root keypair.
    #[must_use]
    pub fn generate() -> Self {
        let keypair = KeyPair::new();
        let public_hex = hex::encode(keypair.public().to_bytes());
        Self { keypair, public_hex }
    }

    /// Persist the private key to disk in `0o600` JSON form.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let private_hex = hex::encode(self.keypair.private().to_bytes());
        let stored = StoredKey { private_hex };
        let json = serde_json::to_string_pretty(&stored).map_err(err)?;
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| err_io(&e))?;
        }
        fs::write(p, json).map_err(|e| err_io(&e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(p, perms);
        }
        Ok(())
    }

    /// Load a previously-generated key from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path.as_ref()).map_err(|e| err_io(&e))?;
        let mut stored: StoredKey = serde_json::from_slice(&bytes).map_err(err)?;
        let priv_bytes = hex::decode(&stored.private_hex).map_err(err)?;
        stored.private_hex.zeroize();
        let private = PrivateKey::from_bytes(&priv_bytes, biscuit_auth::Algorithm::Ed25519)
            .map_err(err)?;
        let keypair = KeyPair::from(&private);
        let public_hex = hex::encode(keypair.public().to_bytes());
        Ok(Self { keypair, public_hex })
    }
}

fn err<E: std::fmt::Display>(e: E) -> PagebridgeError {
    PagebridgeError::Internal(format!("auth: {e}"))
}

fn err_io(e: &std::io::Error) -> PagebridgeError {
    PagebridgeError::Internal(format!("auth io: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("root.key");
        let k = RootKey::generate();
        let pub_hex = k.public_hex.clone();
        k.save(&path).unwrap();
        let loaded = RootKey::load(&path).unwrap();
        assert_eq!(loaded.public_hex, pub_hex);
    }
}
