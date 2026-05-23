#![no_main]

//! Fuzz harness for `DocId::new`.

use libfuzzer_sys::fuzz_target;
use pagebridge_core::DocId;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(id) = DocId::new(s) {
        assert!(id.as_str().len() <= 64);
        assert!(!id.as_str().is_empty());
    }
});
