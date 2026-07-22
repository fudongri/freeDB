use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{AiError, AiProvider, ChatMessage, Role, map_status_error};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url,
            model,
        }
    }

    fn build_messages(messages: &[ChatMessage]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                json!({ "role": role, "content": m.content })
            })
            .collect()
    }

    fn request_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        )
    }
}

/// 通用的请求构建：设置 Authorization 和 Content-Type
fn build_request(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(body)
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), AiError> {
        let body = json!({
            "model": self.model,
            "messages": Self::build_messages(&messages),
            "stream": true,
        });

        let response = build_request(&self.client, &self.request_url(), &self.api_key, &body)
            .send()
            .await
            .map_err(|e| AiError::NetworkError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_status_error(status, text));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| AiError::NetworkError(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // SSE 格式：以 \n 分隔事件（空行分隔不同事件，但 data 行已含完整 JSON）
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        return Ok(());
                    }
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        if let Some(delta) = parsed
                            .get("choices")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("delta"))
                            .and_then(|d| d.get("content"))
                            .and_then(|c| c.as_str())
                        {
                            let _ = tx.send(delta.to_string());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String, AiError> {
        let body = json!({
            "model": self.model,
            "messages": Self::build_messages(&messages),
            "stream": false,
        });

        let response = build_request(&self.client, &self.request_url(), &self.api_key, &body)
            .send()
            .await
            .map_err(|e| AiError::NetworkError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(map_status_error(status, text));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| AiError::InvalidResponse(e.to_string()))?;

        json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AiError::InvalidResponse("missing content in response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_provider(base_url: &str) -> OpenAiProvider {
        OpenAiProvider::new("test-key".into(), base_url.into(), "gpt-4o".into())
    }

    #[test]
    fn build_messages_converts_roles() {
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "You are helpful.".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "Hello".into(),
            },
            ChatMessage {
                role: Role::Assistant,
                content: "Hi!".into(),
            },
        ];
        let result = OpenAiProvider::build_messages(&messages);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["role"], "system");
        assert_eq!(result[0]["content"], "You are helpful.");
        assert_eq!(result[1]["role"], "user");
        assert_eq!(result[1]["content"], "Hello");
        assert_eq!(result[2]["role"], "assistant");
        assert_eq!(result[2]["content"], "Hi!");
    }

    #[test]
    fn build_messages_empty() {
        let result = OpenAiProvider::build_messages(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn request_url_strips_trailing_slash() {
        let p = make_provider("https://api.openai.com/v1/");
        assert_eq!(p.request_url(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn request_url_no_trailing_slash() {
        let p = make_provider("https://api.openai.com/v1");
        assert_eq!(p.request_url(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn map_status_auth_error() {
        let err = map_status_error(StatusCode::UNAUTHORIZED, "unauthorized".into());
        assert!(matches!(err, AiError::AuthError));
    }

    #[test]
    fn map_status_rate_limit() {
        let err = map_status_error(StatusCode::TOO_MANY_REQUESTS, "rate limited".into());
        assert!(matches!(err, AiError::RateLimitExceeded));
    }

    #[test]
    fn map_status_server_error() {
        let err = map_status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal error".into(),
        );
        assert!(matches!(err, AiError::ProviderError(_)));
    }

    #[tokio::test]
    async fn chat_stream_sends_chunks() {
        // 构建包含多个 SSE data 行的 mock 响应
        let sse_body = "\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
data: [DONE]\n\n";

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .create_async()
            .await;

        let provider = make_provider(&server.url());
        let (tx, mut rx) = mpsc::unbounded_channel();

        let result = provider
            .chat_stream(
                vec![ChatMessage {
                    role: Role::User,
                    content: "Hi".into(),
                }],
                tx,
            )
            .await;

        assert!(result.is_ok());
        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            chunks.push(chunk);
        }
        assert_eq!(chunks, vec!["Hello", " world"]);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_returns_content() {
        let response_body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "I am fine, thanks!"
                }
            }]
        });

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response_body.to_string())
            .create_async()
            .await;

        let provider = make_provider(&server.url());
        let result = provider
            .chat(vec![ChatMessage {
                role: Role::User,
                content: "How are you?".into(),
            }])
            .await;

        assert_eq!(result.unwrap(), "I am fine, thanks!");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_auth_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(401)
            .with_body("unauthorized")
            .create_async()
            .await;

        let provider = make_provider(&server.url());
        let result = provider
            .chat(vec![ChatMessage {
                role: Role::User,
                content: "test".into(),
            }])
            .await;

        assert!(matches!(result.unwrap_err(), AiError::AuthError));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_rate_limit() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(429)
            .with_body("rate limited")
            .create_async()
            .await;

        let provider = make_provider(&server.url());
        let result = provider
            .chat(vec![ChatMessage {
                role: Role::User,
                content: "test".into(),
            }])
            .await;

        assert!(matches!(result.unwrap_err(), AiError::RateLimitExceeded));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn chat_missing_content_returns_invalid_response() {
        let bad_body = json!({ "choices": [{}] });

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(bad_body.to_string())
            .create_async()
            .await;

        let provider = make_provider(&server.url());
        let result = provider
            .chat(vec![ChatMessage {
                role: Role::User,
                content: "test".into(),
            }])
            .await;

        assert!(matches!(result.unwrap_err(), AiError::InvalidResponse(_)));
        mock.assert_async().await;
    }
}
