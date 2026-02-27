use super::error::TokenizeError;

/// Common interface for local (offline) token counters.
pub trait Tokenizer {
    /// Short identifier shown in output (e.g. `"o200k"`).
    fn name(&self) -> &'static str;
    /// Count the tokens in `content`.
    fn count_tokens(&self, content: &str) -> Result<usize, TokenizeError>;
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
