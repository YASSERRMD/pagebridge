#![no_main]

//! Fuzz harness for the `SummaryCacheEntry` JSON decoder. Adapters round
//! summary cache entries through serde_json; any panic in decoding would be
//! exposed via this target.

use libfuzzer_sys::fuzz_target;
use pagebridge_core::SummaryCacheEntry;

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<SummaryCacheEntry>(data);
});
