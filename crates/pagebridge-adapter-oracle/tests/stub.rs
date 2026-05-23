//! Stub-mode smoke test for the Oracle adapter.
//!
//! When built without the `oracle-driver` feature, the adapter compiles to a
//! stub whose every method returns an explicit "driver not enabled" error.
//! This test confirms that contract.

#![cfg(not(feature = "oracle-driver"))]

use pagebridge_adapter_oracle::OracleAdapter;
use pagebridge_core::adapter::StorageAdapter;

#[tokio::test]
async fn stub_constructor_reports_disabled() {
    let res = OracleAdapter::connect("u", "p", "//localhost:1521/XEPDB1").await;
    assert!(res.is_err());
    let msg = format!("{}", res.unwrap_err());
    assert!(
        msg.to_lowercase().contains("oracle driver not enabled"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn stub_methods_return_disabled() {
    // The stub itself is constructible directly (the type is unit-like).
    let adapter = OracleAdapter;
    let err = adapter.migrate().await.unwrap_err();
    assert!(format!("{err}").to_lowercase().contains("not enabled"));
}
