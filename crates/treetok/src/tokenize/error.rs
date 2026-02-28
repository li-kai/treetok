/// Error type for tokenization failures.
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum TokenizeError {
    /// Tokenizer could not be initialised.
    #[error("tokenizer init failed: {0}")]
    #[diagnostic(code(treetok::tokenize::init))]
    Init(String),

    /// Neither `TREETOK_API_KEY` nor `ANTHROPIC_API_KEY` is set.
    #[error("API key not found")]
    #[diagnostic(
        code(treetok::tokenize::no_api_key),
        help(
            "set the TREETOK_API_KEY or ANTHROPIC_API_KEY environment variable, or use --offline"
        )
    )]
    NoApiKey,

    /// Claude API rate-limit still hit after all retries.
    #[error("Claude API rate limit exceeded after retries")]
    #[diagnostic(
        code(treetok::tokenize::rate_limit),
        help("wait a moment and try again, or use --offline")
    )]
    RateLimitExceeded,

    /// Claude API returned an unexpected HTTP status.
    #[error("Claude API error (HTTP {status}): {body}")]
    #[diagnostic(code(treetok::tokenize::api_error))]
    ApiError {
        /// HTTP status code from the API.
        status: u16,
        /// Response body from the API.
        body: String,
    },

    /// Network / transport error reaching the Claude API.
    #[error("Claude API network error: {0}")]
    #[diagnostic(code(treetok::tokenize::network))]
    Network(String),

    /// Response JSON was not parseable.
    #[error("Claude API response parse error: {0}")]
    #[diagnostic(code(treetok::tokenize::parse))]
    Parse(String),
}
