use crate::{AiError, AiProvider, ChatMessage, ChatResponse, ChatStreamResult, Role, ToolDef, ToolExecutor};
use tokio::sync::mpsc;

const MAX_TOOL_ROUNDS: usize = 20;

/// 带工具调用的完整对话循环
///
/// - `tx`: `Some` 时同时流式推送文本；`None` 时静默收集
/// - 返回最终完整文本与 reasoning_content
pub async fn chat_with_tools(
    provider: &dyn AiProvider,
    mut messages: Vec<ChatMessage>,
    tools: Vec<ToolDef>,
    executor: &dyn ToolExecutor,
    tx: Option<mpsc::UnboundedSender<String>>,
) -> Result<ChatResponse, AiError> {
    let mut final_text = String::new();
    let mut final_reasoning = String::new();

    for _round in 0..MAX_TOOL_ROUNDS {
        // 有外部流式通道时直接实时转发，避免"先完整接收再排空"导致回答整块出现
        let result = if let Some(outer) = &tx {
            provider
                .chat_stream_with_tools(messages.clone(), tools.clone(), outer.clone())
                .await?
        } else {
            let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
            let res: ChatStreamResult = provider
                .chat_stream_with_tools(messages.clone(), tools.clone(), stream_tx)
                .await?;
            while let Some(chunk) = stream_rx.recv().await {
                final_text.push_str(&chunk);
            }
            res
        };

        if !result.reasoning_content.is_empty() {
            final_reasoning = result.reasoning_content.clone();
        }

        if result.tool_calls.is_empty() {
            return Ok(ChatResponse {
                text: final_text,
                reasoning_content: final_reasoning,
            });
        }

        // 将 assistant 的 tool_calls 追加到消息历史
        messages.push(ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: Some(result.tool_calls.clone()),
            tool_call_id: None,
            reasoning_content: Some(result.reasoning_content.clone()),
        });

        // 执行每个工具并追加结果
        for call in &result.tool_calls {
            let exec_result = executor.execute(&call.name, &call.arguments).await;
            messages.push(ChatMessage::tool_result(&call.id, exec_result));
        }
    }

    Err(AiError::ProviderError(i18n::tr!("工具调用超过最大轮次").to_string()))
}
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_databases".into(),
            description: "列出当前连接中的所有数据库".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDef {
            name: "list_schemas".into(),
            description: "列出指定数据库下的所有 schema（模式）".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "数据库名称"
                    }
                },
                "required": ["database"]
            }),
        },
        ToolDef {
            name: "list_tables".into(),
            description: "列出指定数据库/schema 下的所有表和视图".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "数据库名称（可选）"
                    },
                    "schema": {
                        "type": "string",
                        "description": "schema 名称（可选，MySQL 无需传）"
                    }
                },
                "required": []
            }),
        },
        ToolDef {
            name: "get_table_columns".into(),
            description: "获取指定表的列定义，包括列名、数据类型、是否主键、注释等".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "数据库名称（可选）"
                    },
                    "schema": {
                        "type": "string",
                        "description": "schema 名称（可选）"
                    },
                    "table": {
                        "type": "string",
                        "description": "表名"
                    }
                },
                "required": ["table"]
            }),
        },
        ToolDef {
            name: "search_objects".into(),
            description: "按关键词搜索数据库中的表、视图、存储过程等对象".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "搜索关键词"
                    }
                },
                "required": ["keyword"]
            }),
        },
    ]
}
