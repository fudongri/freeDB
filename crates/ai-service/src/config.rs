use serde::{Deserialize, Serialize};

use crate::AiProviderKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: AiProviderKind,
}

impl AiConfig {
    /// 序列化为 JSON 字符串（用于 save_ui_state）
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 从 JSON 字符串反序列化（用于 load_ui_state）
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

/// OpenAI 兼容格式的预设 Base URL
pub const OPENAI_PRESETS: &[(&str, &str)] = &[
    ("OpenAI", "https://api.openai.com/v1"),
    ("DeepSeek", "https://api.deepseek.com/v1"),
    (
        "通义千问",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
    ),
];

/// Claude API 固定 Base URL
pub const CLAUDE_BASE_URL: &str = "https://api.anthropic.com";
