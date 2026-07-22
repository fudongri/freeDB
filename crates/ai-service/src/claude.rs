use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{AiError, AiProvider, ChatMessage, Role};

pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url,
            model,
        }
    }

    fn request_url(&self) -> String {
        format!(
            "{}/v1/messages",
            self.base_url.trim_end_matches('/')
        )
    }

    fn build_request_body(messages: &[ChatMessage]) -> Value {
        let mut system = String::new();
        let mut api_messages = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    if !system.is_empty() {
                        system.push('\n');
                    }
                    system.push_str(&msg.content);
                }
                Role::User => {
                    api_messages.push(json!({ "role": "user", "content": msg.content }));
                }
                Role::Assistant => {
                    api_messages.push(json!({ "role": "assistant", "content": msg.content }));
                }
            }
        }

        let mut body = json!({
            "model": "",
            "max_tokens": 4096,
            "messages": api_messages,
            "stream": true,
        });

        if !system.is_empty() {
            body["system"] = json!(system);
        }

        body
    }
}

/// 通用的 HTTP 状态码错误处理
fn map_status_error(status: StatusCode, body: String) -> AiError {
    if status == StatusCode::UNAUTHORIZED {
        AiError::AuthError
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        AiError::RateLimitExceeded
    } else {
        AiError::ProviderError(format!("HTTP {}: {}", status, body))
    }
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), AiError> {
        let mut body = Self::build_request_body(&messages);
        body["model"] = json!(self.model);

        let response = self
            .client
            .post(&self.request_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
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

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        let event_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match event_type {
                            "content_block_delta" => {
                                if let Some(text) = parsed
                                    .get("delta")
                                    .and_then(|d| d.get("text"))
                                    .and_then(|t| t.as_str())
                                {
                                    let _ = tx.send(text.to_string());
                                }
                            }
                            "message_stop" => {
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String, AiError> {
        let mut body = Self::build_request_body(&messages);
        body["model"] = json!(self.model);
        body["stream"] = json!(false);

        let response = self
            .client
            .post(&self.request_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
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

        json.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AiError::InvalidResponse("missing content in response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_provider(base_url: &str) -> ClaudeProvider {
        ClaudeProvider::new("test-key".into(), base_url.into(), "claude-sonnet-4-20250514".into())
    }

    #[test]
    fn build_request_body_separates_system() {
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
        let body = ClaudeProvider::build_request_body(&messages);
        assert_eq!(body["system"], "You are helpful.");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "Hi!");
    }

    #[test]
    fn build_request_body_no_system_field_when_absent() {
        let messages = vec![ChatMessage {
            role: Role::User,
            content: "Hi".into(),
        }];
        let body = ClaudeProvider::build_request_body(&messages);
        assert!(body.get("system").is_none());
    }

    #[test]
    fn build_request_body_multiple_system_messages_concatenated() {
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "First instruction.".into(),
            },
            ChatMessage {
                role: Role::System,
                content: "Second instruction.".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "Go".into(),
            },
        ];
        let body = ClaudeProvider::build_request_body(&messages);
        assert_eq!(body["system"], "First instruction.\nSecond instruction.");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn build_request_body_uses_max_tokens() {
        let messages = vec![ChatMessage {
            role: Role::User,
            content: "Hi".into(),
        }];
        let body = ClaudeProvider::build_request_body(&messages);
        assert_eq!(body["max_tokens"], 4096);
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn request_url_strips_trailing_slash() {
        let p = make_provider("https://api.anthropic.com/");
        assert_eq!(p.request_url(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn request_url_no_trailing_slash() {
        let p = make_provider("https://api.anthropic.com");
        assert_eq!(p.request_url(), "https://api.anthropic.com/v1/messages");
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
        let sse_body = "\
event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/messages")
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
            "content": [{
                "type": "text",
                "text": "I am fine, thanks!"
            }]
        });

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/messages")
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
            .mock("POST", "/v1/messages")
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
            .mock("POST", "/v1/messages")
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
}
