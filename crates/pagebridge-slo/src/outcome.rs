use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestOutcome {
    pub latency_ms: u32,
    pub error: bool,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub cost_micro_usd: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HaltSignal {
    /// Caller MAY proceed.
    Proceed,
    /// Caller SHOULD return a partial answer immediately.
    HaltSoft { reason: String },
    /// Caller MUST refuse to start more work.
    HaltHard { reason: String },
}
