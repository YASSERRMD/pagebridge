//! Prompt-injection hardening.
//!
//! Three layered defenses:
//!
//! 1. [`quote_content`]: wrap every leaf with a content-addressed
//!    delimiter the LLM is instructed to treat as data, not as
//!    instructions.
//! 2. [`InstructionDetector`]: regex-based scanner that flags
//!    instruction-like patterns at ingest time. Detected leaves get a
//!    `quarantine` flag.
//! 3. [`sandboxed_synthesis_prompt`]: hardened system prompt that
//!    instructs the LLM to ignore any instructions embedded in the
//!    leaves and rely only on the operator-supplied system prompt.

#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::missing_const_for_fn,
    clippy::iter_on_single_items,
    clippy::needless_collect,
    clippy::too_long_first_doc_paragraph,
    clippy::match_same_arms,
    clippy::redundant_clone,
    clippy::needless_pass_by_value,
    clippy::default_trait_access,
    clippy::useless_vec,
    clippy::unnecessary_wraps,
    clippy::single_match_else,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

pub mod detector;
pub mod prompts;
pub mod redteam;

pub use detector::{Detection, InstructionDetector};
pub use prompts::{quote_content, sandboxed_synthesis_prompt};
pub use redteam::{red_team_set, RedTeamCase};
