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
    ("OpenAI", "https://api.openai.com/v1", "gpt-5.6-sol"),
    ("DeepSeek", "https://api.deepseek.com/v1", "deepseek-v4-flash"),
    ("MiMo", "https://api.xiaomimimo.com/v1", "mimo-v2.5"),
    ("GLM", "https://open.bigmodel.cn/api/paas/v4", "GLM-4.7"),
    ("Kimi", "https://api.moonshot.cn/v1", "K3"),
    (
        "Qwen",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "qwen-max",
    ),
];

/// Claude API 固定 Base URL
pub const CLAUDE_BASE_URL: &str = "https://api.anthropic.com";

/// 各提供商推荐模型列表（名称, Base URL, 模型列表）
pub const PROVIDER_MODELS: &[(&str, &str, &[&str])] = &[
    ("OpenAI", "https://api.openai.com/v1", &["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.5-pro", "gpt-5.3-codex", "o3", "o3-pro", "gpt-4.1"]),
    ("DeepSeek", "https://api.deepseek.com/v1", &["deepseek-v4-flash", "deepseek-v4-pro"]),
    ("Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1", &["qwen-max", "qwen-plus", "qwen-turbo"]),
    ("GLM", "https://open.bigmodel.cn/api/paas/v4", &["GLM-4.7", "GLM-4", "GLM-4-Air", "GLM-4-9B"]),
    ("Kimi", "https://api.moonshot.cn/v1", &["K3", "K2.6", "K3 Swarm"]),
    ("MiMo", "https://api.xiaomimimo.com/v1", &["mimo-v2.5", "mimo-v2.5-pro", "mimo-v2-flash", "mimo-v2-omni"]),
    ("Claude", "https://api.anthropic.com", &["claude-fable-5", "claude-opus-4-8", "claude-sonnet-5", "claude-haiku-4-5"]),
];
