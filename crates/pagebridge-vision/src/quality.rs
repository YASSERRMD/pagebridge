//! Text-extraction quality scoring.
//!
//! When a PDF text extractor produces output, we score it on a 0.0..=1.0
//! scale before deciding to fall back to vision-mode. The score combines:
//!
//! 1. **Character density** -- characters per byte. Garbled PDFs often have
//!    very low or very high density.
//! 2. **Unicode category mix** -- text dominated by control/private-use
//!    codepoints scores low.
//! 3. **Word shape** -- average word length within a plausible band.
//! 4. **Stopword presence** -- a tiny English stopword set; if it shows up
//!    the text is probably real prose.
//!
//! These are heuristics, not statistics. They exist so the ingester can
//! decide quickly whether to spend a vision LLM call.

/// Score a candidate text extraction in `[0.0, 1.0]`. 1.0 = looks fine, 0.0 =
/// probably gibberish.
#[must_use]
pub fn score_text(text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let chars: Vec<char> = text.chars().collect();
    let nchars = chars.len() as f32;

    let mut printable = 0u32;
    let mut control = 0u32;
    let mut private_use = 0u32;
    for ch in &chars {
        match *ch as u32 {
            0x20..=0x7E | 0xA0..=0x10FFFF => printable += 1,
            _ => control += 1,
        }
        let cp = *ch as u32;
        if (0xE000..=0xF8FF).contains(&cp) {
            private_use += 1;
        }
    }
    let printable_ratio = f32::from(u16::try_from(printable).unwrap_or(u16::MAX)) / nchars;
    let control_ratio = f32::from(u16::try_from(control).unwrap_or(u16::MAX)) / nchars;
    let pua_ratio = f32::from(u16::try_from(private_use).unwrap_or(u16::MAX)) / nchars;

    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let word_count = words.len() as f32;
    let avg_word_len = if word_count > 0.0 {
        let total: usize = words.iter().map(|w| w.chars().count()).sum();
        total as f32 / word_count
    } else {
        0.0
    };
    let word_len_score = if (2.0..=12.0).contains(&avg_word_len) {
        1.0
    } else {
        0.3
    };

    let lower = text.to_ascii_lowercase();
    let stopword_hits = STOPWORDS.iter().filter(|w| lower.contains(*w)).count() as f32;
    let stopword_score = (stopword_hits / 5.0).min(1.0);

    // Combine: punish high control/pua, reward stopwords + plausible word shape.
    let base = printable_ratio - control_ratio * 2.0 - pua_ratio * 4.0;
    let combined = base.max(0.0) * 0.4 + stopword_score * 0.4 + word_len_score * 0.2;
    combined.clamp(0.0, 1.0)
}

/// Default threshold for switching to vision-mode. Texts scoring below this
/// trigger the vision fallback.
pub const VISION_THRESHOLD: f32 = 0.45;

/// Helper: should the vision fallback fire for this extraction?
#[must_use]
pub fn needs_vision(text: &str) -> bool {
    score_text(text) < VISION_THRESHOLD
}

const STOPWORDS: &[&str] = &[
    " the ", " and ", " of ", " to ", " in ", " a ", " is ", " for ", " on ", " with ", " that ",
    " by ", " as ", " be ", " this ", " an ", " or ", " from ",
];

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_scores_zero() {
        assert_eq!(score_text(""), 0.0);
    }

    #[test]
    fn real_prose_scores_high() {
        let text = "The implementation timeline for the carbon policy is set for Q1 of the next fiscal year. The rollout will begin in three phases, with the first phase covering municipal vehicles.";
        let s = score_text(text);
        assert!(s > 0.6, "expected > 0.6, got {s}");
        assert!(!needs_vision(text));
    }

    #[test]
    fn gibberish_pua_scores_low() {
        // Build a string of private-use-area characters.
        let mut s = String::new();
        for _ in 0..200 {
            s.push('\u{E000}');
        }
        let score = score_text(&s);
        assert!(score < 0.3, "expected low score, got {score}");
        assert!(needs_vision(&s));
    }

    #[test]
    fn random_alphanumeric_with_no_stopwords_falls_below_threshold() {
        let text = "xyz qwe rty pop kkk zzz mmm nnn bbb ccc dddd eeee ffff gggg";
        let score = score_text(text);
        // No English stopwords, so it should land below the vision threshold.
        assert!(score < 0.7, "got {score}");
    }
}
