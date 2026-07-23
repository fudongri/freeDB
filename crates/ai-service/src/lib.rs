pub mod claude;
pub mod config;
pub mod openai;
pub mod prompt;
pub mod tools;

use tokio::sync::mpsc;

/// 工具定义 — 注册给 AI 的可调用工具
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// AI 请求调用的工具
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 工具执行器 trait — 由 app 层实现，传入 chat_with_tools
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, arguments: &str) -> String;
}

/// 为闭包实现 ToolExecutor
#[async_trait::async_trait]
impl<F, Fut> ToolExecutor for F
where
    F: Fn(String, String) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = String> + Send,
{
    async fn execute(&self, name: &str, arguments: &str) -> String {
        (self)(name.to_string(), arguments.to_string()).await
    }
}

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
    Tool,
}

impl Default for Role {
    fn default() -> Self {
        Role::User
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            ..Default::default()
        }
    }

    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(call_id.into()),
            ..Default::default()
        }
    }
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

    /// 带工具的流式调用：流式输出文本到 tx，流结束后返回 tool_calls（若无则为空 Vec）
    async fn chat_stream_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDef>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<Vec<ToolCall>, AiError> {
        // 默认实现：忽略 tools，走普通流式调用
        let _ = tools;
        self.chat_stream(messages, tx).await?;
        Ok(vec![])
    }
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
