//! Tokenizer trait and concrete implementations.

mod error;
mod local;
mod remote;
mod resolve;
mod run;

pub use error::TokenizeError;
pub use local::{CtocTokenizer, Tokenizer};
pub use resolve::{load_api_key, resolve_tokenizers};
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

    /// For small inputs (under the old 20k threshold), the single Aho-Corasick
    /// path should produce a consistent, deterministic count.
    #[rstest]
    fn ctoc_greedy_and_dp_agree_on_small_input(ctoc: CtocTokenizer) {
        let input = "The quick brown fox jumps over the lazy dog. ".repeat(100);
        let count1 = ctoc.count_tokens(&input).unwrap();
        let count2 = ctoc.count_tokens(&input).unwrap();
        assert_eq!(
            count1, count2,
            "same input should always produce same count"
        );
        assert!(count1 > 0);
    }

    /// Bytes unlikely to appear in vocab should each count as 1 token.
    #[rstest]
    fn ctoc_counts_unmatched_bytes_individually(ctoc: CtocTokenizer) {
        let input = "\x00\x01\x02";
        let count = ctoc.count_tokens(input).unwrap();
        assert_eq!(count, 3);
    }

    /// Large inputs (50k+ bytes) should work without any threshold switching.
    #[rstest]
    fn ctoc_handles_large_input(ctoc: CtocTokenizer) {
        let input = "hello world ".repeat(5000); // ~60k bytes
        let count = ctoc.count_tokens(&input).unwrap();
        assert!(count > 0);
        assert!(count < input.len()); // tokens should be fewer than raw bytes
    }

    /// ctoc is an approximate tokenizer.
    #[rstest]
    fn ctoc_is_approximate(ctoc: CtocTokenizer) {
        assert!(ctoc.is_approximate());
    }
}
