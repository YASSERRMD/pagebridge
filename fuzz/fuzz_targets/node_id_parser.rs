#![no_main]

//! Fuzz harness for `NodeId::new`. Asserts that the parser never panics on
//! arbitrary input and that any value it accepts can be round-tripped via
//! `as_str`.

use libfuzzer_sys::fuzz_target;
use pagebridge_core::NodeId;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let result = NodeId::new(s);
    if let Ok(id) = result {
        let roundtrip = NodeId::new(id.as_str()).expect("re-parse must succeed");
        assert_eq!(id.as_str(), roundtrip.as_str());
    }
});
