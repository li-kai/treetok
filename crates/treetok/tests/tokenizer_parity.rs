//! Parity test: `HuggingFace` Claude-tokenizer candidates vs. Anthropic's
//! `count_tokens` API endpoint, using this repository's own source files —
//! every text file under `crates/` — as the comparison corpus.
//!
//! # Background
//!
//! Anthropic has not released the tokenizer for Claude 3 or later.  All
//! publicly available `HuggingFace` models claiming to be "the Claude tokenizer"
//! derive from the officially released **Claude 1/2** vocabulary
//! (`anthropics/anthropic-tokenizer-typescript`, vocab size 64 739).
//!
//! The best-known independent reverse-engineering effort is **ctoc** (Rohan
//! Gupta, Feb 2026), which probed the `count_tokens` API ~277 000 times to
//! recover 36 495 verified tokens and claims ~96 % accuracy on Claude 4.x.
//! ctoc is Python-only and not available as a `HuggingFace` `tokenizer.json`.
//!
//! Candidates evaluated here (all loadable via the `tokenizers` Rust crate):
//!
//! | id | `HuggingFace` model | Notes |
//! |---|---|---|
//! | `xenova` | `Xenova/claude-tokenizer` | Claude 1/2 verbatim |
//! | `leafspark` | `leafspark/claude-3-tokenizer` | Custom format; loaded speculatively |
//!
//! (`Quivr/claude-tokenizer` is a byte-for-byte clone of Xenova and is
//! omitted.  `BEE-spoke-data` and `pszemraj` add training-only special tokens
//! and are similarly excluded as they add no new signal.)
//!
//! # What the test measures
//!
//! The remote tokenizer (`ClaudeTokenizer`) wraps content in a single-message
//! request body, so the API returns `raw_tokens + overhead` where `overhead`
//! is the fixed number of tokens for the message envelope.  This test:
//!
//! 1. Derives `overhead` from two short reference strings and asserts it is
//!    constant (if this assert fails, the API's token accounting changed).
//! 2. Fetches each candidate tokenizer.  Candidates that 404 or fail to parse
//!    are skipped with a diagnostic rather than aborting the test.
//! 3. For the concatenated corpus, records each candidate's:
//!    `local` count, `adjusted` API count, signed `delta`, and `error %`.
//! 4. Prints a summary table and always succeeds — the output is the result.
//!
//! # Running
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... \
//!   cargo test --test tokenizer_parity -- --include-ignored --nocapture
//! ```
//!
//! The test is `#[ignore]`d by default so CI passes without credentials.

use std::path::PathBuf;

use tokenizers::Tokenizer;
use treetok::{
    tokenize::{CtocTokenizer, Tokenizer as _, resolve_tokenizers},
    walk::{FileKind, WalkOptions, walk_paths},
};

// ── candidate tokenizers ──────────────────────────────────────────────────────

struct Candidate {
    id: &'static str,
    /// Direct URL to the JSON file loadable by the `tokenizers` crate.
    url: &'static str,
}

/// All candidates to compare against the Anthropic API.
///
/// `leafspark/claude-3-tokenizer` ships a custom `tokenizer_config.json`
/// instead of a standard `tokenizer.json`; we attempt to load it speculatively.
const CANDIDATES: &[Candidate] = &[
    Candidate {
        id: "xenova",
        url: "https://huggingface.co/Xenova/claude-tokenizer/resolve/main/tokenizer.json",
    },
    Candidate {
        id: "leafspark",
        url: "https://huggingface.co/leafspark/claude-3-tokenizer/resolve/main/tokenizer_config.json",
    },
];

// ── helpers ───────────────────────────────────────────────────────────────────

/// Attempt to download and parse a tokenizer JSON.  Returns `None` on any
/// network or parse failure (with a diagnostic printed to stderr).
async fn try_fetch_tokenizer(id: &str, url: &str) -> Option<Tokenizer> {
    let resp = match reqwest::get(url).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [{id}] network error fetching {url}: {e}");
            return None;
        }
    };

    if !resp.status().is_success() {
        eprintln!(
            "  [{id}] HTTP {} for {url} — skipping",
            resp.status().as_u16()
        );
        return None;
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  [{id}] error reading body: {e}");
            return None;
        }
    };

    match Tokenizer::from_bytes(&bytes) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("  [{id}] failed to parse tokenizer JSON: {e}");
            None
        }
    }
}

fn local_count(tok: &Tokenizer, text: &str) -> usize {
    #[allow(clippy::expect_used)] // helper only called from test fns; panic is correct behaviour
    tok.encode(text, false)
        .expect("local tokenizer encode failed")
        .get_ids()
        .len()
}

// ── test ──────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires ANTHROPIC_API_KEY (or TREETOK_API_KEY) and network access"]
async fn hf_claude_tokenizer_candidates_vs_api() {
    const REF1: &str = "a";
    const REF2: &str = "hello world";

    // ── remote tokenizer ─────────────────────────────────────────────────────

    let resolved = resolve_tokenizers(&["claude".to_string()], false)
        .expect("failed to resolve claude tokenizer – is an API key set?");
    let remote = resolved
        .claude
        .as_ref()
        .expect("ClaudeTokenizer unavailable (check ANTHROPIC_API_KEY / TREETOK_API_KEY)");

    // ── envelope overhead ────────────────────────────────────────────────────
    //
    // Derived from two reference strings using the Xenova tokenizer (Claude
    // 1/2 vocabulary), which assigns exactly 1 token to "a" and 2 to "hello
    // world".  The overhead must be identical for both; if it isn't the API's
    // message-framing changed.

    let xenova = try_fetch_tokenizer("xenova (overhead probe)", CANDIDATES[0].url)
        .await
        .expect("Xenova tokenizer must be available to establish envelope overhead");

    let ref1_remote: usize = remote
        .count_tokens(REF1)
        .await
        .expect("count_tokens(REF1) failed");
    let overhead = ref1_remote - local_count(&xenova, REF1);

    let ref2_remote: usize = remote
        .count_tokens(REF2)
        .await
        .expect("count_tokens(REF2) failed");
    let overhead2 = ref2_remote - local_count(&xenova, REF2);
    assert_eq!(
        overhead, overhead2,
        "message-envelope overhead is not constant: REF1→{overhead}, REF2→{overhead2}"
    );

    eprintln!("\nAPI message-envelope overhead: {overhead} token(s)");

    // ── corpus: every text file under crates/ ────────────────────────────────

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent missing")
        .parent()
        .expect("repo root parent missing")
        .to_path_buf();

    let walk = walk_paths(
        &[repo_root.join("crates")],
        &WalkOptions {
            no_ignore: false,
            depth: None,
        },
    );

    let text_files: Vec<(PathBuf, String)> = walk
        .into_iter()
        .flat_map(|r| r.entries)
        .filter(|e| matches!(e.kind, FileKind::Text))
        .filter_map(|e| e.content.map(|c| (e.rel_path, c)))
        .collect();

    assert!(!text_files.is_empty(), "no text files found under crates/");

    eprintln!("\ncorpus: {} file(s)", text_files.len());
    for (path, _) in &text_files {
        eprintln!("  {}", path.display());
    }

    let corpus: String = text_files
        .iter()
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // ── remote corpus count (shared baseline) ────────────────────────────────

    let remote_corpus: usize = remote
        .count_tokens(&corpus)
        .await
        .expect("count_tokens(corpus) failed");
    let adjusted = remote_corpus - overhead;

    eprintln!(
        "\nRemote (source of truth): {remote_corpus} raw tokens  \
         (−{overhead} envelope = {adjusted} net)\n"
    );

    // ── evaluate each candidate ───────────────────────────────────────────────

    eprintln!(
        "{:<12} {:>7} {:>8} {:>8} {:>8}",
        "candidate", "local", "adjusted", "delta", "error%"
    );
    eprintln!("{}", "-".repeat(49));

    for candidate in CANDIDATES {
        let Some(tok) = try_fetch_tokenizer(candidate.id, candidate.url).await else {
            eprintln!("{:<12}  (skipped — tokenizer unavailable)", candidate.id);
            continue;
        };

        let local = local_count(&tok, &corpus);
        let delta = (local as i64) - (adjusted as i64);
        let pct = delta as f64 / adjusted as f64 * 100.0;

        eprintln!(
            "{:<12} {:>7} {:>8} {:>8} {:>7.1}%",
            candidate.id, local, adjusted, delta, pct
        );
    }

    // ── ctoc (Rohan Gupta, Feb 2026) ─────────────────────────────────────────
    {
        let ctoc = CtocTokenizer::new();
        let local = ctoc
            .count_tokens(&corpus)
            .expect("ctoc count_tokens failed");
        let delta = (local as i64) - (adjusted as i64);
        let pct = delta as f64 / adjusted as f64 * 100.0;
        eprintln!(
            "{:<12} {:>7} {:>8} {:>8} {:>7.1}%",
            "ctoc", local, adjusted, delta, pct
        );
    }

    eprintln!();
}
