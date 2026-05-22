//! Cognitive query engine: candidate selection, LLM-guided navigation, synthesis.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::doc_markdown,
    clippy::single_match_else,
    clippy::similar_names,
    clippy::if_not_else,
    clippy::manual_let_else,
    clippy::format_push_string,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::format_push_string,
    clippy::redundant_clone,
    clippy::assigning_clones,
    clippy::needless_borrows_for_generic_args,
    clippy::doc_overindented_list_items,
    clippy::ignored_unit_patterns,
    clippy::too_long_first_doc_paragraph,
    clippy::unused_self,
    clippy::cognitive_complexity,
    clippy::too_many_arguments
)]

pub mod candidates;
pub mod navigate;
pub mod synthesize;

pub use navigate::{navigate, NavigationOutcome};
pub use synthesize::synthesize_answer;
