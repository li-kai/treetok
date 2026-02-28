use super::error::TokenizeError;
use super::local::{CtocTokenizer, O200kTokenizer, Tokenizer};
use super::remote::{self, ClaudeTokenizer};

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
/// * `explicit`  – names from `-t` flags (empty = use all available).
/// * `offline`   – if `true`, online tokenizers are skipped even if a key is present.
/// * `api_key`   – pre-resolved API key (`None` = unavailable or offline).
pub fn resolve_tokenizers(
    explicit: &[String],
    offline: bool,
    api_key: Option<String>,
) -> Result<ResolvedTokenizers, TokenizeError> {
    if explicit.is_empty() {
        // Range mode: use all available tokenizers.
        let mut local: Vec<Box<dyn Tokenizer>> = vec![Box::new(O200kTokenizer::new()?)];

        let claude = match (offline, api_key) {
            (false, Some(key)) => Some(ClaudeTokenizer::with_key(key)),
            (false, None) => {
                eprintln!(
                    "note: ANTHROPIC_API_KEY not set \u{2014} \
                     using ctoc (embedded approximation, \u{2248}96\u{a0}% accurate vs Claude API). \
                     Set ANTHROPIC_API_KEY for exact counts."
                );
                None
            }
            (true, _) => None,
        };

        // Fall back to ctoc whenever the Claude API tokenizer is unavailable.
        if claude.is_none() {
            local.push(Box::new(CtocTokenizer::new()));
        }

        Ok(ResolvedTokenizers { local, claude })
    } else {
        // Explicit mode: instantiate only what was requested.
        let mut local: Vec<Box<dyn Tokenizer>> = Vec::new();
        let mut claude: Option<ClaudeTokenizer> = None;

        for name in explicit {
            match name.as_str() {
                "o200k" => local.push(Box::new(O200kTokenizer::new()?)),
                "ctoc" => local.push(Box::new(CtocTokenizer::new())),
                "claude" => match (offline, api_key.clone()) {
                    (true, _) => eprintln!("warning: --offline set, skipping -t claude"),
                    (false, Some(key)) => claude = Some(ClaudeTokenizer::with_key(key)),
                    (false, None) => return Err(TokenizeError::NoApiKey),
                },
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

/// Load the API key from the environment for use with [`resolve_tokenizers`].
///
/// Prefers `TREETOK_API_KEY` over `ANTHROPIC_API_KEY`; returns `None` if
/// neither is set.
pub fn load_api_key() -> Option<String> {
    remote::load_api_key().ok()
}
