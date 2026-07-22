pub mod claude;
pub mod config;
pub mod openai;
pub mod prompt;

use tokio::sync::mpsc;

/// 将 HTTP 状态码映射为 AiError
pub fn map_status_error(status: reqwest::StatusCode, body: String) -> AiError {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        AiError::AuthError
    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        AiError::RateLimitExceeded
    } else {
        AiError::ProviderError(format!("HTTP {}: {}", status, body))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("API Key 无效或权限不足")]
    AuthError,
    #[error("请求频率超限，请稍后重试")]
    RateLimitExceeded,
    #[error("响应解析失败: {0}")]
    InvalidResponse(String),
    #[error("AI 服务错误: {0}")]
    ProviderError(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AiProviderKind {
    OpenAI {
        api_key: String,
        base_url: String,
        model: String,
    },
    Claude {
        api_key: String,
        base_url: String,
        model: String,
    },
}

#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// 流式发送消息，通过 channel 返回文本片段
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<(), AiError>;

    /// 非流式发送，返回完整回复
    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String, AiError>;
}

/// 根据配置创建对应的 AI Provider 实例
pub fn create_provider(kind: &AiProviderKind) -> Box<dyn AiProvider> {
    match kind {
        AiProviderKind::OpenAI {
            api_key,
            base_url,
            model,
        } => Box::new(openai::OpenAiProvider::new(
            api_key.clone(),
            base_url.clone(),
            model.clone(),
        )),
        AiProviderKind::Claude {
            api_key,
            base_url,
            model,
        } => Box::new(claude::ClaudeProvider::new(
            api_key.clone(),
            base_url.clone(),
            model.clone(),
        )),
    }
}
