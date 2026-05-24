use async_trait::async_trait;
use parking_lot::Mutex;

use crate::scorer::ScoreSample;

#[async_trait]
pub trait QualityStore: Send + Sync + 'static {
    async fn append(&self, sample: ScoreSample);
    async fn since(&self, ts_ns: u128) -> Vec<ScoreSample>;
}

pub struct MemoryQualityStore {
    inner: Mutex<Vec<ScoreSample>>,
}

impl MemoryQualityStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryQualityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QualityStore for MemoryQualityStore {
    async fn append(&self, sample: ScoreSample) {
        self.inner.lock().push(sample);
    }
    async fn since(&self, ts_ns: u128) -> Vec<ScoreSample> {
        self.inner
            .lock()
            .iter()
            .filter(|s| s.ts_ns >= ts_ns)
            .cloned()
            .collect()
    }
}
