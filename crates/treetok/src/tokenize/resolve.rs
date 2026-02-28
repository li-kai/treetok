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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn no_key() -> Option<String> { None }
    fn some_key() -> Option<String> { Some("test-key".to_string()) }

    #[rstest]
    #[case::offline(true,  some_key())]
    #[case::no_api_key(false, no_key())]
    fn range_mode_without_claude_gives_o200k_and_ctoc(
        #[case] offline: bool,
        #[case] api_key: Option<String>,
    ) {
        let r = resolve_tokenizers(&[], offline, api_key).unwrap();
        assert_eq!(r.local.len(), 2);
        assert_eq!(r.local[0].name(), "o200k");
        assert_eq!(r.local[1].name(), "ctoc");
        assert!(r.claude.is_none());
    }

    #[test]
    fn range_mode_with_key_uses_claude() {
        let r = resolve_tokenizers(&[], false, some_key()).unwrap();
        assert_eq!(r.local.len(), 1);
        assert_eq!(r.local[0].name(), "o200k");
        assert!(r.claude.is_some());
    }

    #[rstest]
    #[case::o200k("o200k", false)]
    #[case::ctoc("ctoc",   true)]
    fn explicit_local_tokenizer(
        #[case] name: &str,
        #[case] is_approx: bool,
        #[values(false, true)] offline: bool,
    ) {
        let r = resolve_tokenizers(&[name.to_string()], offline, no_key()).unwrap();
        assert_eq!(r.local.len(), 1);
        assert_eq!(r.local[0].name(), name);
        assert_eq!(r.local[0].is_approximate(), is_approx);
        assert!(r.claude.is_none());
    }

    #[test]
    fn explicit_o200k_and_ctoc() {
        let r =
            resolve_tokenizers(&["o200k".to_string(), "ctoc".to_string()], false, no_key())
                .unwrap();
        assert_eq!(r.local.len(), 2);
        assert_eq!(r.local[0].name(), "o200k");
        assert_eq!(r.local[1].name(), "ctoc");
        assert!(r.claude.is_none());
    }

    #[test]
    fn unknown_alongside_valid_is_skipped() {
        let r =
            resolve_tokenizers(&["o200k".to_string(), "unknown".to_string()], false, no_key())
                .unwrap();
        assert_eq!(r.local.len(), 1);
        assert_eq!(r.local[0].name(), "o200k");
    }

    #[rstest]
    #[case::unknown_only(&["not_a_real_tokenizer"][..], false, no_key())]
    #[case::offline_skips_claude(&["claude"][..],       true,  some_key())]
    #[case::no_key_for_claude(&["claude"][..],          false, no_key())]
    fn invalid_explicit_combinations_are_errors(
        #[case] names: &[&str],
        #[case] offline: bool,
        #[case] api_key: Option<String>,
    ) {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        assert!(resolve_tokenizers(&names, offline, api_key).is_err());
    }
}
