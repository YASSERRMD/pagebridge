//! Tests for the vision-mode helpers.

#![allow(
    clippy::assertions_on_constants,
    clippy::manual_repeat_n,
    clippy::manual_string_new,
    clippy::float_cmp
)]

use pagebridge_core::llm::VisionImage;
use pagebridge_vision::quality::{needs_vision, score_text, VISION_THRESHOLD};
use pagebridge_vision::{EchoVisionProvider, VisionProvider};
use serde_json::json;

#[test]
fn quality_threshold_constant_is_in_range() {
    assert!(VISION_THRESHOLD > 0.0 && VISION_THRESHOLD < 1.0);
}

#[test]
fn needs_vision_matches_score_text() {
    let bad: String = "\u{E000}".repeat(200);
    assert!(needs_vision(&bad));
    let good = "The implementation timeline for the rollout is set for the next fiscal year.";
    assert!(!needs_vision(good));
    assert!(score_text(good) > 0.5);
}

#[tokio::test]
async fn echo_vision_provider_returns_canned_output() {
    let p = EchoVisionProvider::new();
    p.push(json!({
        "title": "Carbon policy",
        "sections": [{"heading": "Timeline", "body": "Q1"}],
        "tables": [],
        "figures": []
    }));
    let img = VisionImage {
        bytes: vec![0xFFu8; 32],
        media_type: "image/png".into(),
    };
    let out = p.describe_page(&img).await.unwrap();
    assert_eq!(out["title"], "Carbon policy");
    assert_eq!(out["sections"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn echo_vision_provider_default_when_empty() {
    let p = EchoVisionProvider::default();
    let img = VisionImage {
        bytes: vec![],
        media_type: "image/png".into(),
    };
    let out = p.describe_page(&img).await.unwrap();
    assert_eq!(out["title"], "Untitled");
}
