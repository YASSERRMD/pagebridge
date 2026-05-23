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

#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

pub mod detector;
pub mod prompts;
pub mod redteam;

pub use detector::{Detection, InstructionDetector};
pub use prompts::{quote_content, sandboxed_synthesis_prompt};
pub use redteam::{red_team_set, RedTeamCase};
