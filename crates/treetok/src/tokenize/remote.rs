use super::error::TokenizeError;

const COUNT_TOKENS_URL: &str = "https://api.anthropic.com/v1/messages/count_tokens";
const CLAUDE_MODEL: &str = "claude-sonnet-4-6";
const MAX_RETRIES: u32 = 3;

#[derive(serde::Serialize)]
struct CountTokensRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 1],
}

#[derive(serde::Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Deserialize)]
struct CountTokensResponse {
    input_tokens: usize,
}

/// Online tokenizer that calls Anthropic's `count_tokens` endpoint.
pub struct ClaudeTokenizer {
    api_key: String,
    client: reqwest::Client,
}

/// Select the API key from two candidates, preferring the first.
/// Returns an error if both are unavailable.
fn select_api_key(
    preferred: Option<String>,
    fallback: Option<String>,
) -> Result<String, TokenizeError> {
    preferred.or(fallback).ok_or(TokenizeError::NoApiKey)
}

/// Load API key from environment, preferring `TREETOK_API_KEY` over `ANTHROPIC_API_KEY`.
fn load_api_key() -> Result<String, TokenizeError> {
    let preferred = std::env::var("TREETOK_API_KEY").ok();
    let fallback = std::env::var("ANTHROPIC_API_KEY").ok();
    select_api_key(preferred, fallback)
}

impl ClaudeTokenizer {
    /// Create a new tokenizer.  Returns `Err(TokenizeError::NoApiKey)` if
    /// neither `TREETOK_API_KEY` nor `ANTHROPIC_API_KEY` is set.
    /// Prefers `TREETOK_API_KEY` if both are set.
    pub fn new() -> Result<Self, TokenizeError> {
        let api_key = load_api_key()?;
        let client = reqwest::Client::new();
        Ok(Self { api_key, client })
    }

    /// Count tokens via the Anthropic API (async, with retry on 429).
    pub async fn count_tokens(&self, content: &str) -> Result<usize, TokenizeError> {
        let body = CountTokensRequest {
            model: CLAUDE_MODEL,
            messages: [Message {
                role: "user",
                content,
            }],
        };

        for attempt in 0..MAX_RETRIES {
            let resp = self
                .client
                .post(COUNT_TOKENS_URL)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| TokenizeError::Network(e.to_string()))?;

            let status = resp.status().as_u16();

            if status == 200 {
                let parsed: CountTokensResponse = resp
                    .json()
                    .await
                    .map_err(|e| TokenizeError::Parse(e.to_string()))?;
                return Ok(parsed.input_tokens);
            } else if status == 429 {
                let wait = std::time::Duration::from_secs(1 << attempt);
                tokio::time::sleep(wait).await;
            } else {
                let body_text = resp.text().await.unwrap_or_default();
                return Err(TokenizeError::ApiError {
                    status,
                    body: body_text,
                });
            }
        }

        Err(TokenizeError::RateLimitExceeded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(Some("treetok-key".to_string()), Some("anthropic-key".to_string()), "treetok-key")]
    #[case(None, Some("anthropic-key".to_string()), "anthropic-key")]
    #[case(Some("preferred".to_string()), Some("fallback".to_string()), "preferred")]
    fn select_api_key_returns_correct_key(
        #[case] preferred: Option<String>,
        #[case] fallback: Option<String>,
        #[case] expected: &str,
    ) {
        let result = select_api_key(preferred, fallback);
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn select_api_key_errors_when_both_missing() {
        let result: Result<String, TokenizeError> = select_api_key(None, None);
        assert!(matches!(result.unwrap_err(), TokenizeError::NoApiKey));
    }
}
