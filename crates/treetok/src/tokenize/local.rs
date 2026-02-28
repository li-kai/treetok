use std::collections::HashSet;

use super::error::TokenizeError;

/// Common interface for local (offline) token counters.
pub trait Tokenizer {
    /// Short identifier shown in output (e.g. `"o200k"`).
    fn name(&self) -> &'static str;
    /// Count the tokens in `content`.
    fn count_tokens(&self, content: &str) -> Result<usize, TokenizeError>;
    /// Returns `true` if this tokenizer produces approximate counts.
    fn is_approximate(&self) -> bool {
        false
    }
}

// ─── o200k (tiktoken) ────────────────────────────────────────────────────────

/// Offline tokenizer backed by `OpenAI`'s `o200k_base` BPE vocabulary.
pub struct O200kTokenizer {
    bpe: tiktoken_rs::CoreBPE,
}

impl O200kTokenizer {
    /// Initialise the encoder (loads vocabulary data).
    pub fn new() -> Result<Self, TokenizeError> {
        let bpe = tiktoken_rs::o200k_base().map_err(|e| TokenizeError::Init(e.to_string()))?;
        Ok(Self { bpe })
    }
}

impl Tokenizer for O200kTokenizer {
    fn name(&self) -> &'static str {
        "o200k"
    }

    fn count_tokens(&self, content: &str) -> Result<usize, TokenizeError> {
        Ok(self.bpe.encode_ordinary(content).len())
    }
}

// ─── ctoc (Rohan Gupta, Feb 2026 reverse-engineering) ────────────────────────

/// Embedded vocab binary.  Updated by `cargo xtask update-ctoc` / `just update-ctoc`.
///
/// Format: a sequence of `[length: u16 LE][token_bytes: u8 × length]` records.
static CTOC_VOCAB: &[u8] = include_bytes!("ctoc_vocab.bin");

/// Offline approximation of Claude's tokenizer based on the *ctoc* reverse-
/// engineering project (Rohan Gupta, Feb 2026).  Covers ~36 495 verified
/// tokens and claims ~96 % accuracy on Claude 4.x.
///
/// The vocabulary is embedded at compile time; use `cargo xtask update-ctoc`
/// (or `just update-ctoc`) to refresh it from the upstream source.
pub struct CtocTokenizer {
    vocab: HashSet<Vec<u8>>,
    max_len: usize,
}

impl CtocTokenizer {
    /// Build the tokenizer from the embedded vocab.
    pub fn new() -> Self {
        let mut vocab = HashSet::new();
        let mut max_len = 0usize;
        let data = CTOC_VOCAB;
        let mut pos = 0;
        while pos + 2 <= data.len() {
            let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + len > data.len() {
                break;
            }
            max_len = max_len.max(len);
            vocab.insert(data[pos..pos + len].to_vec());
            pos += len;
        }
        Self { vocab, max_len }
    }
}

impl Tokenizer for CtocTokenizer {
    fn name(&self) -> &'static str {
        "ctoc"
    }

    fn is_approximate(&self) -> bool {
        true
    }

    fn count_tokens(&self, text: &str) -> Result<usize, TokenizeError> {
        let bytes = text.as_bytes();
        let n = bytes.len();
        let mut dp = vec![usize::MAX; n + 1];
        dp[0] = 0;
        for i in 0..n {
            if dp[i] == usize::MAX {
                continue;
            }
            let end = (i + self.max_len).min(n);
            for len in 1..=(end - i) {
                if self.vocab.contains(&bytes[i..i + len]) {
                    dp[i + len] = dp[i + len].min(dp[i] + 1);
                }
            }
            // unknown byte fallback
            dp[i + 1] = dp[i + 1].min(dp[i] + 1);
        }
        Ok(dp[n])
    }
}
