use super::remote::ClaudeTokenizer;
use super::error::TokenizeError;
use super::local::{O200kTokenizer, Tokenizer};

/// Split tokenizer set: local (synchronous) and optional Claude (async).
pub struct ResolvedTokenizers {
    /// Offline tokenizers (o200k, etc.).
    pub local: Vec<Box<dyn Tokenizer>>,
    /// Optional Claude API tokenizer.
    pub claude: Option<ClaudeTokenizer>,
}

impl ResolvedTokenizers {
    /// Total number of active tokenizers.
    pub fn count(&self) -> usize {
        self.local.len() + usize::from(self.claude.is_some())
    }
}

/// Decide which tokenizers to activate.
///
/// * `explicit` – names from `-t` flags (empty = use all available).
/// * `offline`  – if `true`, online tokenizers are skipped even if an API key
///   is present.
pub fn resolve_tokenizers(
    explicit: &[String],
    offline: bool,
) -> Result<ResolvedTokenizers, TokenizeError> {
    if explicit.is_empty() {
        // Range mode: use all available tokenizers.
        let local: Vec<Box<dyn Tokenizer>> = vec![Box::new(O200kTokenizer::new()?)];

        let claude = if offline {
            None
        } else {
            match ClaudeTokenizer::new() {
                Ok(t) => Some(t),
                Err(TokenizeError::NoApiKey) => None,
                Err(e) => return Err(e),
            }
        };

        Ok(ResolvedTokenizers { local, claude })
    } else {
        // Explicit mode: instantiate only what was requested.
        let mut local: Vec<Box<dyn Tokenizer>> = Vec::new();
        let mut claude: Option<ClaudeTokenizer> = None;

        for name in explicit {
            match name.as_str() {
                "o200k" => local.push(Box::new(O200kTokenizer::new()?)),
                "claude" => {
                    if offline {
                        eprintln!("warning: --offline set, skipping -t claude");
                    } else {
                        claude = Some(ClaudeTokenizer::new()?);
                    }
                }
                other => {
                    eprintln!("warning: unknown tokenizer {other:?}, skipping");
                }
            }
        }

        if local.is_empty() && claude.is_none() {
            return Err(TokenizeError::Init(
                "no valid tokenizers available".to_string(),
            ));
        }

        Ok(ResolvedTokenizers { local, claude })
    }
}
