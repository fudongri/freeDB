use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{AiError, AiProvider, ChatMessage};

pub struct ClaudeProvider {
    _api_key: String,
    _model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            _api_key: api_key,
            _model: model,
        }
    }
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn chat_stream(
        &self,
        _messages: Vec<ChatMessage>,
        _tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), AiError> {
        todo!("Task 3: Claude API 流式实现")
    }

    async fn chat(&self, _messages: Vec<ChatMessage>) -> Result<String, AiError> {
        todo!("Task 3: Claude API 流式实现")
    }
}
