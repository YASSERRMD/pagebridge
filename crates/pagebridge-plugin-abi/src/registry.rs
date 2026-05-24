//! Process-wide registry of installed plugins.
//!
//! The registry holds owned copies of every plugin manifest, plus the source
//! location (filesystem path for dylib, or a WASM module path). Higher-level
//! loaders (libloading, wasmtime) install entries here so the CLI can list
//! and inspect them.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::abi::{OwnedManifest, PluginKind, ABI_VERSION};

/// Where the plugin lives on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Dylib(PathBuf),
    Wasm(PathBuf),
}

/// One installed plugin entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub manifest: OwnedManifest,
    pub source: PluginSource,
    /// SHA-256 of the source file at install time, used to detect mutation.
    pub source_hash: String,
    /// True if the entry passed a signature check at install time.
    pub trusted: bool,
}

/// Process-wide registry. Plugin loaders install entries; the CLI reads them.
#[derive(Default, Clone)]
pub struct PluginRegistry {
    inner: Arc<RwLock<Vec<PluginEntry>>>,
}

/// Reason a manifest was rejected.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("plugin ABI version mismatch: expected {ABI_VERSION}, got {0}")]
    AbiMismatch(u32),
    #[error("plugin with name {0:?} already installed")]
    Duplicate(String),
}

impl PluginRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a plugin manifest. Validates the ABI version and rejects
    /// duplicate names.
    pub fn install(&self, entry: PluginEntry) -> Result<(), InstallError> {
        if entry.manifest.abi_version != ABI_VERSION {
            return Err(InstallError::AbiMismatch(entry.manifest.abi_version));
        }
        let mut guard = self.inner.write();
        if guard.iter().any(|e| e.manifest.name == entry.manifest.name) {
            return Err(InstallError::Duplicate(entry.manifest.name));
        }
        guard.push(entry);
        Ok(())
    }

    /// Remove an installed plugin by name. Returns true if it existed.
    pub fn remove(&self, name: &str) -> bool {
        let mut guard = self.inner.write();
        let before = guard.len();
        guard.retain(|e| e.manifest.name != name);
        guard.len() < before
    }

    /// Snapshot of every installed plugin.
    #[must_use]
    pub fn list(&self) -> Vec<PluginEntry> {
        self.inner.read().clone()
    }

    /// Fetch one plugin by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<PluginEntry> {
        self.inner
            .read()
            .iter()
            .find(|e| e.manifest.name == name)
            .cloned()
    }

    /// Convenience: count by kind.
    #[must_use]
    pub fn count_kind(&self, kind: PluginKind) -> usize {
        self.inner
            .read()
            .iter()
            .filter(|e| e.manifest.plugin_kind() == kind)
            .count()
    }
}
