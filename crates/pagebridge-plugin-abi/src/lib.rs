//! Plugin ABI and registry for pagebridge.
//!
//! Two layers:
//!
//! 1. [`abi`]: the C ABI types every plugin exposes (`PluginManifest`,
//!    `PluginKind`, `ABI_VERSION`). Stable across host versions.
//! 2. [`registry`]: a process-wide `PluginRegistry` that tracks installed
//!    plugins. Loaders (libloading for dylibs, wasmtime for WASM) install
//!    entries here so the CLI can list and remove them.
//!
//! v1.0.0 ships the ABI surface, registry, and CLI plumbing. Real dylib and
//! WASM loaders land alongside the plugin author guide in the v1.1 ecosystem
//! release; both layer additively on this foundation.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_safety_doc,
    clippy::unsafe_derive_deserialize,
    clippy::significant_drop_in_scrutinee,
    clippy::significant_drop_tightening
)]

pub mod abi;
pub mod registry;

pub use abi::{OwnedManifest, PluginKind, PluginManifest, ABI_VERSION};
pub use registry::{InstallError, PluginEntry, PluginRegistry, PluginSource};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str) -> PluginEntry {
        PluginEntry {
            manifest: OwnedManifest {
                abi_version: ABI_VERSION,
                kind: PluginKind::Adapter as u8,
                name: name.to_owned(),
                vendor: "test".into(),
                version: "0.1.0".into(),
            },
            source: PluginSource::Dylib(PathBuf::from("/tmp/x.so")),
            source_hash: "abc".into(),
            trusted: false,
        }
    }

    #[test]
    fn install_then_list_returns_entry() {
        let reg = PluginRegistry::new();
        reg.install(make_entry("foo")).unwrap();
        assert_eq!(reg.list().len(), 1);
        assert!(reg.get("foo").is_some());
    }

    #[test]
    fn duplicate_install_is_rejected() {
        let reg = PluginRegistry::new();
        reg.install(make_entry("foo")).unwrap();
        let err = reg.install(make_entry("foo")).unwrap_err();
        assert!(matches!(err, InstallError::Duplicate(_)));
    }

    #[test]
    fn abi_mismatch_is_rejected() {
        let reg = PluginRegistry::new();
        let mut entry = make_entry("foo");
        entry.manifest.abi_version = 99;
        let err = reg.install(entry).unwrap_err();
        assert!(matches!(err, InstallError::AbiMismatch(99)));
    }

    #[test]
    fn remove_returns_true_when_present() {
        let reg = PluginRegistry::new();
        reg.install(make_entry("foo")).unwrap();
        assert!(reg.remove("foo"));
        assert!(!reg.remove("foo"));
    }

    #[test]
    fn count_kind_filters_correctly() {
        let reg = PluginRegistry::new();
        reg.install(make_entry("adapter1")).unwrap();
        let mut entry = make_entry("provider1");
        entry.manifest.kind = PluginKind::LlmProvider as u8;
        reg.install(entry).unwrap();
        assert_eq!(reg.count_kind(PluginKind::Adapter), 1);
        assert_eq!(reg.count_kind(PluginKind::LlmProvider), 1);
    }
}
