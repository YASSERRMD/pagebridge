//! Stable C ABI types exchanged between pagebridge and its plugins.
//!
//! Plugins compile as either a dynamic library exposing
//! `pagebridge_plugin_manifest` returning `*const PluginManifest`, or as a
//! WASM module exporting the same symbol. The host loads the manifest first,
//! validates the ABI version, then dispatches the appropriate constructor.

use std::os::raw::c_char;

/// Current ABI version. Bump whenever the in-memory layout of the manifest
/// changes; plugins compiled against an older version are rejected.
pub const ABI_VERSION: u32 = 1;

/// What sort of plugin this is.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Adapter = 0,
    LlmProvider = 1,
}

/// Manifest pointer returned by `pagebridge_plugin_manifest()`. Strings are
/// nul-terminated UTF-8.
#[repr(C)]
pub struct PluginManifest {
    pub abi_version: u32,
    pub plugin_kind: u8,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub version: *const c_char,
}

/// Safe owned copy of a `PluginManifest`. Use [`from_raw`] to construct one
/// from a `*const PluginManifest` returned by a plugin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OwnedManifest {
    pub abi_version: u32,
    pub kind: u8,
    pub name: String,
    pub vendor: String,
    pub version: String,
}

impl OwnedManifest {
    /// Copy a manifest pointer into owned strings. Safe because we copy out
    /// of the foreign-owned memory immediately.
    ///
    /// # Safety
    /// `ptr` must point to a valid `PluginManifest` with valid nul-terminated
    /// UTF-8 strings.
    #[must_use]
    pub unsafe fn from_raw(ptr: *const PluginManifest) -> Option<Self> {
        if ptr.is_null() {
            return None;
        }
        let raw = &*ptr;
        let read = |p: *const c_char| -> Option<String> {
            if p.is_null() {
                return None;
            }
            std::ffi::CStr::from_ptr(p)
                .to_str()
                .ok()
                .map(str::to_owned)
        };
        Some(Self {
            abi_version: raw.abi_version,
            kind: raw.plugin_kind,
            name: read(raw.name).unwrap_or_default(),
            vendor: read(raw.vendor).unwrap_or_default(),
            version: read(raw.version).unwrap_or_default(),
        })
    }

    /// Convenience accessor for the kind.
    #[must_use]
    pub const fn plugin_kind(&self) -> PluginKind {
        match self.kind {
            0 => PluginKind::Adapter,
            _ => PluginKind::LlmProvider,
        }
    }
}
