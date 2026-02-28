//! Tokenizer trait and concrete implementations.

mod remote;
mod error;
mod local;
mod resolve;
mod run;

pub use error::TokenizeError;
pub use local::{CtocTokenizer, Tokenizer};
pub use resolve::resolve_tokenizers;
pub use run::tokenize_entries;

#[cfg(test)]
use local::O200kTokenizer;

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};

    // ── fixtures ───────────────────────────────────────────────────────────

    #[fixture]
    fn o200k() -> O200kTokenizer {
        O200kTokenizer::new().expect("o200k init should succeed")
    }

    // ── O200kTokenizer ─────────────────────────────────────────────────────

    /// Empty string produces zero tokens.
    #[rstest]
    fn empty_string_is_zero_tokens(o200k: O200kTokenizer) {
        assert_eq!(o200k.count_tokens("").unwrap(), 0);
    }

    /// Non-empty inputs all produce at least one token.
    #[rstest]
    #[case("a")]
    #[case("hello")]
    #[case("Hello, world!")]
    #[case("fn main() { println!(\"hi\"); }")]
    fn nonempty_input_has_positive_count(o200k: O200kTokenizer, #[case] input: &str) {
        assert!(o200k.count_tokens(input).unwrap() > 0);
    }

    /// A longer text produces more tokens than a shorter one.
    #[rstest]
    fn longer_text_has_more_tokens(o200k: O200kTokenizer) {
        let short = o200k.count_tokens("hi").unwrap();
        let long = o200k
            .count_tokens(
                "hello world this is a longer sentence with many more words and tokens in it",
            )
            .unwrap();
        assert!(long > short);
    }


    // ── CtocTokenizer ──────────────────────────────────────────────────────

    #[fixture]
    fn ctoc() -> CtocTokenizer {
        CtocTokenizer::new()
    }

    #[rstest]
    fn ctoc_empty_string_is_zero_tokens(ctoc: CtocTokenizer) {
        assert_eq!(ctoc.count_tokens("").unwrap(), 0);
    }

    #[rstest]
    #[case("a")]
    #[case("hello")]
    #[case("Hello, world!")]
    #[case("fn main() { println!(\"hi\"); }")]
    fn ctoc_nonempty_input_has_positive_count(ctoc: CtocTokenizer, #[case] input: &str) {
        assert!(ctoc.count_tokens(input).unwrap() > 0);
    }

    #[rstest]
    fn ctoc_longer_text_has_more_tokens(ctoc: CtocTokenizer) {
        let short = ctoc.count_tokens("hi").unwrap();
        let long = ctoc
            .count_tokens(
                "hello world this is a longer sentence with many more words and tokens in it",
            )
            .unwrap();
        assert!(long > short);
    }

    // ── resolve_tokenizers ─────────────────────────────────────────────────

    /// `--offline` with no explicit flags → exactly one tokenizer (o200k).
    #[test]
    fn resolve_offline_gives_only_o200k() {
        let resolved = resolve_tokenizers(&[], true).unwrap();
        assert_eq!(resolved.local.len(), 1);
        assert_eq!(resolved.local[0].name(), "o200k");
        assert!(resolved.claude.is_none());
    }

    /// Explicit `-t o200k` → one local tokenizer named "o200k".
    #[test]
    fn resolve_explicit_o200k() {
        let resolved = resolve_tokenizers(&["o200k".to_string()], false).unwrap();
        assert_eq!(resolved.local.len(), 1);
        assert_eq!(resolved.local[0].name(), "o200k");
        assert!(resolved.claude.is_none());
    }

    /// Explicit `-t ctoc` → one local approximate tokenizer named "ctoc".
    #[test]
    fn resolve_explicit_ctoc() {
        let resolved = resolve_tokenizers(&["ctoc".to_string()], false).unwrap();
        assert_eq!(resolved.local.len(), 1);
        assert_eq!(resolved.local[0].name(), "ctoc");
        assert!(resolved.local[0].is_approximate());
        assert!(resolved.claude.is_none());
    }

    /// When no API key is set, range mode includes ctoc as an approximation.
    #[test]
    fn resolve_no_api_key_includes_ctoc() {
        // Ensure neither key is set for this test.
        let treetok_key = std::env::var("TREETOK_API_KEY").ok();
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
        if treetok_key.is_some() || anthropic_key.is_some() {
            // Skip: an API key is present, so ctoc fallback won't activate.
            return;
        }
        let resolved = resolve_tokenizers(&[], false).unwrap();
        assert!(
            resolved.local.iter().any(|t| t.name() == "ctoc"),
            "expected ctoc in local tokenizers when no API key is set"
        );
        assert!(resolved.claude.is_none());
    }

    /// Requesting only an unknown tokenizer should return an error.
    #[test]
    fn resolve_only_unknown_is_error() {
        let result = resolve_tokenizers(&["not_a_real_tokenizer".to_string()], false);
        assert!(result.is_err());
    }

    /// Mixing a valid and unknown name should succeed (unknown is skipped).
    #[test]
    fn resolve_valid_and_unknown_succeeds() {
        let resolved =
            resolve_tokenizers(&["o200k".to_string(), "unknown".to_string()], false).unwrap();
        assert_eq!(resolved.local.len(), 1);
        assert_eq!(resolved.local[0].name(), "o200k");
    }
}
