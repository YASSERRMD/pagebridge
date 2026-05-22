//! Python bindings for pagebridge. Full PyO3 wiring lands in Phase 16.

/// Returns the underlying core crate version.
#[must_use]
pub const fn version() -> &'static str {
    pagebridge_core::version()
}
