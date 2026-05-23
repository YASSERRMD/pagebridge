//! Streaming variant of `synthesize_answer`.
//!
//! The synthesis prompt asks the model to emit `[[CITE:<node_id>]]` markers
//! inline as it references leaves. This module wraps the model's token stream,
//! parses out those markers, resolves each to a `Citation`, and emits a mix of
//! `Token` and `Citation` `AnswerChunk`s.
//!
//! The terminal chunk is `AnswerChunk::Done`, carrying the consolidated trace
//! and citation list.

use std::sync::Arc;

use async_stream::try_stream;
use futures::{Stream, StreamExt};

use crate::adapter::StorageAdapter;
use crate::citation;
use crate::error::{PagebridgeError, Result};
use crate::llm::{ChatMessage, CompletionRequest, LlmProvider, StreamChunk};
use crate::prompts::{PromptContext, PromptLibrary};
use crate::record::NodeRecord;
use crate::trace::TraceBuilder;
use crate::types::AnswerChunk;

/// Stream a cited answer. Yields `Token`, `Citation`, and a final `Done` chunk.
pub async fn synthesize_answer_stream(
    storage: Arc<dyn StorageAdapter>,
    llm: Arc<dyn LlmProvider>,
    prompts: Arc<PromptLibrary>,
    question: String,
    leaves: Vec<NodeRecord>,
    mut trace: TraceBuilder,
) -> Result<impl Stream<Item = Result<AnswerChunk>> + Send> {
    // Hydrate leaf bodies before we hand the prompt to the model. This mirrors
    // the non-streaming path so citation resolution sees the same leaves.
    let mut hydrated = Vec::with_capacity(leaves.len());
    let mut total_chars = 0usize;
    for mut leaf in leaves {
        if let Some(span) = leaf.span {
            if let Ok(text) = storage.read_raw_text(&leaf.doc_id, span).await {
                if !text.trim().is_empty() {
                    leaf.summary = text;
                }
            }
        }
        total_chars += leaf.summary.chars().count();
        hydrated.push(leaf);
    }
    trace.synthesis_start(hydrated.len(), total_chars);

    let leaves_snapshot = hydrated.clone();
    let ctx = PromptContext {
        question: Some(question),
        leaves: hydrated,
        ..PromptContext::default()
    };
    let prompt = prompts.render("synthesize", &ctx)?;
    let req = CompletionRequest {
        system: Some(
            "Answer only using the provided leaves. Emit citations inline as \
             [[CITE:<node_id>]] markers as soon as you reference a leaf."
                .into(),
        ),
        messages: vec![ChatMessage::user(prompt)],
        ..CompletionRequest::default()
    };
    let token_stream = llm.complete_stream(req).await?;
    Ok(citation_aware(
        storage,
        token_stream,
        leaves_snapshot,
        trace,
    ))
}

/// Adapt a raw token stream into an `AnswerChunk` stream by parsing the inline
/// citation markers and emitting them as `AnswerChunk::Citation` events.
fn citation_aware(
    storage: Arc<dyn StorageAdapter>,
    mut tokens: crate::llm::CompletionStream,
    leaves: Vec<NodeRecord>,
    mut trace: TraceBuilder,
) -> impl Stream<Item = Result<AnswerChunk>> + Send {
    try_stream! {
        let mut parser = MarkerParser::new();
        let mut total_input = 0u32;
        let mut total_output = 0u32;
        let mut emitted_ids: Vec<crate::id::NodeId> = Vec::new();

        while let Some(item) = tokens.next().await {
            let chunk = item?;
            match chunk {
                StreamChunk::Token(text) => {
                    for piece in parser.consume(&text) {
                        match piece {
                            Piece::Text(t) if !t.is_empty() => {
                                yield AnswerChunk::Token { text: t };
                            }
                            Piece::Marker(id) => {
                                if let Ok(node_id) = crate::id::NodeId::new(id.clone()) {
                                    let single = std::slice::from_ref(&node_id);
                                    let mut citations = citation::build_citations(
                                        &storage,
                                        &leaves,
                                        single,
                                    )
                                    .await;
                                    if let Some(citation) = citations.pop() {
                                        emitted_ids.push(node_id);
                                        yield AnswerChunk::Citation { citation };
                                    }
                                }
                            }
                            Piece::Text(_) => {}
                        }
                    }
                }
                StreamChunk::Finished {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    total_input = input_tokens;
                    total_output = output_tokens;
                }
            }
        }
        // Flush any trailing text held in the parser buffer.
        for piece in parser.flush() {
            if let Piece::Text(t) = piece {
                if !t.is_empty() {
                    yield AnswerChunk::Token { text: t };
                }
            }
        }
        trace.synthesis_done(total_input, total_output, 0);
        trace.final_citations(&emitted_ids);
        let consolidated =
            citation::build_citations(&storage, &leaves, &emitted_ids).await;
        trace.finish();
        yield AnswerChunk::Done {
            trace: trace.clone_data(),
            citations: consolidated,
        };
    }
}

#[derive(Debug, Clone)]
enum Piece {
    Text(String),
    Marker(String),
}

/// Stateful parser that splits a streamed string into `Text` runs and
/// `Marker(node_id)` events. Handles markers that span chunk boundaries by
/// holding back any trailing partial-marker prefix until enough bytes arrive.
#[derive(Default)]
struct MarkerParser {
    buffer: String,
}

impl MarkerParser {
    fn new() -> Self {
        Self::default()
    }

    fn consume(&mut self, input: &str) -> Vec<Piece> {
        self.buffer.push_str(input);
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buffer.find("[[CITE:") else {
                // No marker start visible. If the tail of the buffer could be
                // the start of a marker, hold back enough bytes.
                let safe = safe_emit_len(&self.buffer);
                if safe > 0 {
                    let text = self.buffer[..safe].to_owned();
                    self.buffer.drain(..safe);
                    out.push(Piece::Text(text));
                }
                break;
            };
            if start > 0 {
                let text = self.buffer[..start].to_owned();
                self.buffer.drain(..start);
                out.push(Piece::Text(text));
            }
            let Some(rel_end) = self.buffer.find("]]") else {
                // Marker not yet terminated; wait for more input.
                break;
            };
            let prefix_len = "[[CITE:".len();
            let id = self.buffer[prefix_len..rel_end].to_owned();
            self.buffer.drain(..rel_end + 2);
            out.push(Piece::Marker(id));
        }
        out
    }

    fn flush(mut self) -> Vec<Piece> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let leftover = std::mem::take(&mut self.buffer);
        vec![Piece::Text(leftover)]
    }
}

/// Length of `buf` we can safely emit without prematurely cutting a partial
/// `[[CITE:` prefix. We hold back at most `len("[[CITE:") - 1` trailing bytes.
#[allow(clippy::missing_const_for_fn)]
fn safe_emit_len(buf: &str) -> usize {
    const MARKER: &str = "[[CITE:";
    let hold = MARKER.len() - 1;
    if buf.len() <= hold {
        return 0;
    }
    let candidate = buf.len() - hold;
    // Walk back to the nearest char boundary to avoid splitting a UTF-8 codepoint.
    let mut idx = candidate;
    while !buf.is_char_boundary(idx) && idx > 0 {
        idx -= 1;
    }
    idx
}

/// Build a fully-buffered `Answer` by consuming an [`AnswerChunk`] stream to
/// completion. Useful for callers that want streaming under the hood but a
/// single value at the surface (e.g. the CLI without `--stream`).
pub async fn collect_stream(
    mut stream: impl Stream<Item = Result<AnswerChunk>> + Unpin + Send,
) -> Result<crate::types::Answer> {
    let mut text = String::new();
    let mut citations = Vec::new();
    let mut trace = None;
    while let Some(chunk) = stream.next().await {
        match chunk? {
            AnswerChunk::Token { text: t } => text.push_str(&t),
            AnswerChunk::Citation { citation } => citations.push(citation),
            AnswerChunk::Done {
                trace: t,
                citations: cs,
            } => {
                trace = Some(t);
                if citations.is_empty() {
                    citations = cs;
                }
            }
        }
    }
    let trace = trace.ok_or_else(|| {
        PagebridgeError::Internal("stream ended without Done chunk".into())
    })?;
    Ok(crate::types::Answer {
        text,
        citations,
        trace,
    })
}

#[cfg(test)]
mod tests {
    use super::{MarkerParser, Piece};

    fn texts(pieces: Vec<Piece>) -> Vec<String> {
        pieces
            .into_iter()
            .filter_map(|p| match p {
                Piece::Text(t) => Some(t),
                Piece::Marker(_) => None,
            })
            .collect()
    }
    fn markers(pieces: Vec<Piece>) -> Vec<String> {
        pieces
            .into_iter()
            .filter_map(|p| match p {
                Piece::Marker(m) => Some(m),
                Piece::Text(_) => None,
            })
            .collect()
    }

    #[test]
    fn parser_emits_plain_text() {
        let mut p = MarkerParser::new();
        let pieces = p.consume("hello world");
        // "hello wor" can be emitted, "ld" is held until we know it isn't the
        // start of a marker.
        assert!(texts(pieces).join("").starts_with("hello"));
        let flushed = p.flush();
        assert!(texts(flushed).join("").contains("ld"));
    }

    #[test]
    fn parser_finds_complete_marker_in_one_chunk() {
        let mut p = MarkerParser::new();
        let pieces = p.consume("before[[CITE:doc:a/sec:1/leaf:0]]after");
        let mut all = pieces;
        all.extend(p.flush());
        let m = markers(all.clone());
        assert_eq!(m, vec!["doc:a/sec:1/leaf:0".to_owned()]);
        let joined: String = texts(all).join("");
        assert!(joined.contains("before"));
        assert!(joined.contains("after"));
    }

    #[test]
    fn parser_handles_marker_split_across_chunks() {
        let mut p = MarkerParser::new();
        let mut all = p.consume("first[[CITE:doc:");
        all.extend(p.consume("a/sec:1/leaf:0]]tail"));
        all.extend(p.flush());
        let m = markers(all.clone());
        assert_eq!(m, vec!["doc:a/sec:1/leaf:0".to_owned()]);
        assert!(texts(all).join("").contains("first"));
    }
}
