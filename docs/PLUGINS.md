# Authoring pagebridge plugins

Plugins extend pagebridge with custom storage adapters and LLM providers
without forking the workspace. v1.0.0 ships the stable C ABI surface and
the in-process registry; the dynamic-library and WASM loaders ride on the
same surface and ship alongside the v1.1 ecosystem release.

## The ABI

Every plugin exports a single C entry point that returns a pointer to a
`PluginManifest`:

```c
struct PluginManifest {
    uint32_t abi_version;       // must equal pagebridge_plugin_abi::ABI_VERSION
    uint8_t  plugin_kind;       // 0 = adapter, 1 = llm_provider
    const char* name;           // unique within the registry
    const char* vendor;
    const char* version;        // SemVer
};

const struct PluginManifest* pagebridge_plugin_manifest(void);
```

Rust implementations declare it directly:

```rust
use pagebridge_plugin_abi::{PluginManifest, ABI_VERSION};
use std::os::raw::c_char;

static NAME: &[u8] = b"my-adapter\0";
static VENDOR: &[u8] = b"acme\0";
static VERSION: &[u8] = b"0.1.0\0";

static MANIFEST: PluginManifest = PluginManifest {
    abi_version: ABI_VERSION,
    plugin_kind: 0,
    name: NAME.as_ptr() as *const c_char,
    vendor: VENDOR.as_ptr() as *const c_char,
    version: VERSION.as_ptr() as *const c_char,
};

#[no_mangle]
pub extern "C" fn pagebridge_plugin_manifest() -> *const PluginManifest {
    &MANIFEST
}
```

## The registry

`pagebridge::plugin::PluginRegistry` is the host-side store of installed
plugins. The CLI lists what's registered via `pagebridge plugins list` and
reports the supported ABI version via `pagebridge plugins abi-version`.

Loader crates (libloading for dylibs, wasmtime for WASM) call
`PluginRegistry::install` with a `PluginEntry` after validating the manifest.
Install fails on:

- ABI mismatch (the manifest's `abi_version` does not equal `ABI_VERSION`).
- Duplicate name (a plugin with the same `name` is already registered).

## Roadmap

- `pagebridge-plugin-dylib`: libloading-based loader with optional signature
  verification.
- `pagebridge-plugin-wasm`: wasmtime sandbox loader exposing a host
  interface (`read`, `write`, `log`, `sleep`, `http_fetch`).
- A worked example for each kind (custom KV adapter, custom embedding LLM
  wrapper).
