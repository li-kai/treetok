use std::collections::BTreeMap;
use std::sync::Arc;

use futures::stream::{self, StreamExt};

use super::remote::ClaudeTokenizer;
use super::resolve::ResolvedTokenizers;
use super::TokenizerId;
use crate::output::TokenCount;
use crate::walk::FileKind;

/// Tokenize a slice of file entries and return results.
///
/// Phase 1: run local tokenizers sequentially (microseconds per file).
/// Phase 2: run Claude tokenizer asynchronously with concurrency limiting.
#[must_use]
pub fn tokenize_entries(
    entries: &[crate::walk::FileEntry],
    tokenizers: &ResolvedTokenizers,
) -> Vec<crate::output::FileResult> {
    // Phase 1: local tokenizers (sequential).
    let mut results: Vec<crate::output::FileResult> = entries
        .iter()
        .map(|entry| {
            let (kind, tokens) = match &entry.kind {
                FileKind::Binary => (FileKind::Binary, BTreeMap::new()),
                FileKind::TooLarge => (FileKind::TooLarge, BTreeMap::new()),
                FileKind::Error(msg) => {
                    eprintln!("warning: {}: {msg}", entry.path.display());
                    (FileKind::Error(msg.clone()), BTreeMap::new())
                }
                FileKind::Text => {
                    let content = entry.content.as_deref().unwrap_or("");
                    let mut counts: BTreeMap<TokenizerId, TokenCount> = BTreeMap::new();

                    for tok in &tokenizers.local {
                        match tok.count_tokens(content) {
                            Ok(n) => {
                                let tc = if tok.is_approximate() {
                                    TokenCount::from_approx(n)
                                } else {
                                    TokenCount::Exact(n)
                                };
                                counts.insert(tok.id(), tc);
                            }
                            Err(e) => {
                                eprintln!(
                                    "warning: {} [{}]: {e}",
                                    entry.path.display(),
                                    tok.id().as_str()
                                );
                            }
                        }
                    }

                    (FileKind::Text, counts)
                }
            };

            crate::output::FileResult {
                rel_path: entry.rel_path.clone(),
                kind,
                tokens,
            }
        })
        .collect();

    // Phase 2: Claude (async, concurrency-limited).
    if let Some(claude) = &tokenizers.claude {
        #[allow(clippy::expect_used)] // Infallible in practice; no recovery path.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(claude_tokenize_all(&mut results, entries, claude));
    }

    results
}

async fn claude_tokenize_all(
    results: &mut [crate::output::FileResult],
    entries: &[crate::walk::FileEntry],
    claude: &ClaudeTokenizer,
) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(20));

    let text_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, crate::walk::FileKind::Text))
        .map(|(i, _)| i)
        .collect();

    let counts: Vec<_> = stream::iter(text_indices)
        .map(|idx| {
            let sem = semaphore.clone();
            let content = entries[idx].content.as_deref().unwrap_or("");
            async move {
                #[allow(clippy::expect_used)] // Semaphore is never closed.
                let _permit = sem.acquire().await.expect("semaphore closed");
                (idx, claude.count_tokens(content).await)
            }
        })
        .buffer_unordered(20)
        .collect()
        .await;

    for (idx, result) in counts {
        match result {
            Ok(n) => {
                results[idx]
                    .tokens
                    .insert(TokenizerId::Claude, TokenCount::Exact(n));
            }
            Err(e) => {
                eprintln!(
                    "warning: {} [{}]: {e}",
                    entries[idx].path.display(),
                    TokenizerId::Claude.as_str()
                );
            }
        }
    }
}
