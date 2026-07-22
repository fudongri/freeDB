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

/// 单个 AI 模型配置条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModelEntry {
    pub name: String,
    pub config: AiConfig,
}

/// 多模型配置管理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModelStore {
    pub models: Vec<AiModelEntry>,
    pub active_index: usize,
}

impl Default for AiModelStore {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            active_index: 0,
        }
    }
}

impl AiModelStore {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Self {
        serde_json::from_str(s).ok().unwrap_or_default()
    }

    pub fn active(&self) -> Option<&AiConfig> {
        self.models.get(self.active_index).map(|e| &e.config)
    }

    pub fn add(&mut self, name: String, config: AiConfig) {
        self.models.push(AiModelEntry { name, config });
        self.active_index = self.models.len() - 1;
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.models.len() {
            self.models.remove(index);
            if self.active_index >= self.models.len() && !self.models.is_empty() {
                self.active_index = self.models.len() - 1;
            }
        }
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.models.len() {
            self.active_index = index;
        }
    }
}

/// OpenAI 兼容格式的预设 (名称, Base URL, 默认模型)
pub const OPENAI_PRESETS: &[(&str, &str, &str)] = &[
    ("OpenAI", "https://api.openai.com/v1", "gpt-4.1"),
    ("DeepSeek", "https://api.deepseek.com/v1", "deepseek-v4-flash"),
    ("MiMo", "https://api.xiaomimimo.com/v1", "mimo-v2-pro"),
    ("GLM", "https://open.bigmodel.cn/api/paas/v4", "glm-4-flash"),
    ("Kimi", "https://api.moonshot.cn/v1", "moonshot-v1-auto"),
    (
        "Qwen",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "qwen-plus",
    ),
];

/// Claude API 固定 Base URL
pub const CLAUDE_BASE_URL: &str = "https://api.anthropic.com";

/// 各提供商推荐模型列表（名称, Base URL, 模型列表）
pub const PROVIDER_MODELS: &[(&str, &str, &[&str])] = &[
    ("OpenAI", "https://api.openai.com/v1", &["gpt-4.1", "gpt-4.1-mini", "gpt-4.1-nano", "o3", "o3-mini", "o4-mini"]),
    ("DeepSeek", "https://api.deepseek.com/v1", &["deepseek-v4-flash", "deepseek-v4-pro"]),
    ("MiMo", "https://api.xiaomimimo.com/v1", &["mimo-v2-pro", "mimo-v2-flash"]),
    ("GLM", "https://open.bigmodel.cn/api/paas/v4", &["glm-4-plus", "glm-4-flash", "glm-4-long"]),
    ("Kimi", "https://api.moonshot.cn/v1", &["moonshot-v1-auto", "moonshot-v1-128k", "moonshot-v1-32k"]),
    ("Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1", &["qwen-plus", "qwen-turbo", "qwen-max", "qwen3-coder-plus"]),
    ("Claude", "", &["claude-sonnet-4-20250514", "claude-opus-4-20250514", "claude-haiku-4-5-20251001"]),
];
