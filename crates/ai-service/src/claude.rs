use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{AiError, AiProvider, ChatMessage, ChatStreamResult, Role, ToolCall, ToolDef, map_status_error};

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
        let mut api_messages: Vec<Value> = Vec::new();

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
                    if let Some(ref tool_calls) = msg.tool_calls {
                        // 包含 tool_use 的 assistant 消息
                        let mut content: Vec<Value> = Vec::new();
                        if !msg.content.is_empty() {
                            content.push(json!({ "type": "text", "text": msg.content }));
                        }
                        for tc in tool_calls {
                            let input: Value = serde_json::from_str(&tc.arguments)
                                .unwrap_or(serde_json::json!({}));
                            content.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": input
                            }));
                        }
                        api_messages.push(json!({ "role": "assistant", "content": content }));
                    } else {
                        api_messages.push(json!({ "role": "assistant", "content": msg.content }));
                    }
                }
                Role::Tool => {
                    // 工具结果作为 user 消息的 tool_result 内容块
                    let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                    api_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": msg.content
                        }]
                    }));
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

    async fn chat_stream_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDef>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<ChatStreamResult, AiError> {
        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect();

        let mut body = Self::build_request_body(&messages);
        body["model"] = json!(self.model);
        body["tools"] = json!(tools_json);

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

        // 当前正在累积的 tool_use 块
        struct ActiveToolUse {
            id: String,
            name: String,
            arguments: String,
        }

        let mut active_tool: Option<ActiveToolUse> = None;
        let mut completed_tools: Vec<ToolCall> = Vec::new();

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
                        let event_type =
                            parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match event_type {
                            "content_block_start" => {
                                if let Some(block) = parsed.get("content_block") {
                                    let block_type =
                                        block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if block_type == "tool_use" {
                                        active_tool = Some(ActiveToolUse {
                                            id: block
                                                .get("id")
                                                .and_then(|i| i.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            name: block
                                                .get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            arguments: String::new(),
                                        });
                                    }
                                }
                            }
                            "content_block_delta" => {
                                if let Some(delta) = parsed.get("delta") {
                                    let delta_type =
                                        delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    match delta_type {
                                        "text_delta" => {
                                            if let Some(text) =
                                                delta.get("text").and_then(|t| t.as_str())
                                            {
                                                let _ = tx.send(text.to_string());
                                            }
                                        }
                                        "input_json_delta" => {
                                            if let Some(partial) = delta
                                                .get("partial_json")
                                                .and_then(|p| p.as_str())
                                            {
                                                if let Some(ref mut tool) = active_tool {
                                                    tool.arguments.push_str(partial);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            "content_block_stop" => {
                                if let Some(tool) = active_tool.take() {
                                    completed_tools.push(ToolCall {
                                        id: tool.id,
                                        name: tool.name,
                                        arguments: tool.arguments,
                                    });
                                }
                            }
                            "message_stop" => {
                                return Ok(ChatStreamResult {
                                    tool_calls: completed_tools,
                                    reasoning_content: String::new(),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(ChatStreamResult {
            tool_calls: completed_tools,
            reasoning_content: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_provider(base_url: &str) -> ClaudeProvider {
        ClaudeProvider::new("test-key".into(), base_url.into(), "claude-sonnet-5".into())
    }

    #[test]
    fn build_request_body_separates_system() {
        let messages = vec![
            ChatMessage::new(Role::System, "You are helpful.".into(),),
            ChatMessage::new(Role::User, "Hello".into(),),
            ChatMessage::new(Role::Assistant, "Hi!".into(),),
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
        let messages = vec![ChatMessage::new(Role::User, "Hi".into(),)];
        let body = ClaudeProvider::build_request_body(&messages);
        assert!(body.get("system").is_none());
    }

    #[test]
    fn build_request_body_multiple_system_messages_concatenated() {
        let messages = vec![
            ChatMessage::new(Role::System, "First instruction.".into(),),
            ChatMessage::new(Role::System, "Second instruction.".into(),),
            ChatMessage::new(Role::User, "Go".into(),),
        ];
        let body = ClaudeProvider::build_request_body(&messages);
        assert_eq!(body["system"], "First instruction.\nSecond instruction.");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn build_request_body_uses_max_tokens() {
        let messages = vec![ChatMessage::new(Role::User, "Hi".into(),)];
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
                vec![ChatMessage::new(Role::User, "Hi".into(),)],
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
            .chat(vec![ChatMessage::new(Role::User, "How are you?".into(),)])
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
            .chat(vec![ChatMessage::new(Role::User, "test".into(),)])
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
            .chat(vec![ChatMessage::new(Role::User, "test".into(),)])
            .await;

        assert!(matches!(result.unwrap_err(), AiError::RateLimitExceeded));
        mock.assert_async().await;
    }
}
