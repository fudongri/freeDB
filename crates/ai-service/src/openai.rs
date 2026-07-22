use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{AiError, AiProvider, ChatMessage};

pub struct OpenAiProvider {
    _api_key: String,
    _base_url: String,
    _model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            _api_key: api_key,
            _base_url: base_url,
            _model: model,
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat_stream(
        &self,
        _messages: Vec<ChatMessage>,
        _tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), AiError> {
        todo!("Task 2: OpenAI 兼容格式流式实现")
    }

    async fn chat(&self, _messages: Vec<ChatMessage>) -> Result<String, AiError> {
        todo!("Task 2: OpenAI 兼容格式流式实现")
    }
}
