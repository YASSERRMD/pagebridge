//! Hot reload of configuration without restarting the process.
//!
//! Wraps any `T: Send + Sync + 'static` in a [`Hot<T>`] handle. Readers
//! load the current value lock-free; the producer publishes a new value
//! atomically. In-flight queries see the value that was current when
//! they started.

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

pub mod prompts;
pub mod source;

pub use source::{ConfigSource, FileSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadEvent {
    pub at_ns: u128,
    pub kind: String,
    pub version: u64,
}

/// Lock-free, atomically swappable handle around any `T`.
pub struct Hot<T> {
    inner: ArcSwap<T>,
    version: std::sync::atomic::AtomicU64,
}

impl<T: Send + Sync + 'static> Hot<T> {
    pub fn new(initial: T) -> Self {
        Self {
            inner: ArcSwap::from(Arc::new(initial)),
            version: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Load the currently active value. Lock-free.
    pub fn load(&self) -> Arc<T> {
        self.inner.load_full()
    }

    /// Publish a new value. Atomic.
    pub fn swap(&self, next: T) -> Arc<T> {
        self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.swap(Arc::new(next))
    }

    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_advances_version_and_value() {
        let h: Hot<String> = Hot::new("v1".to_string());
        assert_eq!(*h.load(), "v1");
        h.swap("v2".to_string());
        assert_eq!(*h.load(), "v2");
        assert_eq!(h.version(), 2);
    }
}
