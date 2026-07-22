use crate::ChatMessage;
use crate::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiAction {
    Optimize,
    Generate,
    Explain,
    DataQuality,
    DataAnalysis,
}

/// 构建系统提示词，包含数据库类型和表结构信息
pub fn system_prompt(db_type: &str, schema_info: &str) -> String {
    format!(
        "你是一个数据库专家助手。当前使用的数据库是 {}。\n\
         以下是相关表/集合的结构信息：\n{}\n\n\
         请用中文回复。回答要简洁专业。",
        db_type, schema_info
    )
}

/// 构建完整的对话消息：system + user
pub fn build_messages(system: &str, user_prompt: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage { role: Role::System, content: system.to_string() },
        ChatMessage { role: Role::User, content: user_prompt.to_string() },
    ]
}

pub fn optimize_prompt(statement: &str) -> String {
    format!(
        "请优化以下数据库查询语句的性能。说明优化原因和预期效果。\n\n当前语句：\n{}",
        statement
    )
}

pub fn generate_prompt(description: &str) -> String {
    format!(
        "根据以下描述，生成对应的数据库查询语句。只返回语句，不需要解释。\n\n描述：\n{}",
        description
    )
}

pub fn explain_prompt(statement: &str) -> String {
    format!(
        "请解释以下数据库查询语句的含义，用通俗易懂的语言说明：\n\
         1. 这条语句做了什么\n\
         2. 查询了哪些表/集合\n\
         3. 关键条件和逻辑\n\n\
         语句：\n{}",
        statement
    )
}

pub fn data_quality_prompt(statement: &str, row_count: usize, data_sample: &str) -> String {
    format!(
        "请检查以下数据的质量问题，包括但不限于：\n\
         - 空值/NULL 比例\n\
         - 重复数据\n\
         - 异常值\n\
         - 数据类型不一致\n\
         - 格式问题\n\n\
         查询语句：{}\n\
         数据（前 {} 行）：\n{}",
        statement, row_count, data_sample
    )
}

pub fn data_analysis_prompt(statement: &str, row_count: usize, data_sample: &str) -> String {
    format!(
        "请对以下数据进行分析，提供：\n\
         1. 数据概览（行数、列数）\n\
         2. 各列的统计特征\n\
         3. 关键发现和趋势\n\
         4. 潜在的业务洞察\n\n\
         查询语句：{}\n\
         数据（前 {} 行）：\n{}",
        statement, row_count, data_sample
    )
}

/// 测试连接用的简单提示词
pub fn test_connection_prompt() -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: Role::User,
            content: "请回复\"连接成功\"两个字。".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_contains_db_type_and_schema() {
        let prompt = system_prompt("MySQL", "users(id INT, name VARCHAR)");
        assert!(prompt.contains("MySQL"));
        assert!(prompt.contains("users(id INT, name VARCHAR)"));
        assert!(prompt.contains("数据库专家"));
    }

    #[test]
    fn build_messages_returns_system_then_user() {
        let msgs = build_messages("system msg", "user msg");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[0].content, "system msg");
        assert_eq!(msgs[1].role, Role::User);
        assert_eq!(msgs[1].content, "user msg");
    }

    #[test]
    fn optimize_prompt_contains_statement() {
        let prompt = optimize_prompt("SELECT * FROM t");
        assert!(prompt.contains("SELECT * FROM t"));
        assert!(prompt.contains("优化"));
    }

    #[test]
    fn generate_prompt_contains_description() {
        let prompt = generate_prompt("查询所有活跃用户");
        assert!(prompt.contains("查询所有活跃用户"));
        assert!(prompt.contains("生成"));
    }

    #[test]
    fn explain_prompt_contains_statement() {
        let prompt = explain_prompt("SELECT id FROM users WHERE active = 1");
        assert!(prompt.contains("SELECT id FROM users WHERE active = 1"));
        assert!(prompt.contains("解释"));
    }

    #[test]
    fn data_quality_prompt_contains_params() {
        let prompt = data_quality_prompt("SELECT * FROM orders", 10, "id, amount\n1, 100");
        assert!(prompt.contains("SELECT * FROM orders"));
        assert!(prompt.contains("10"));
        assert!(prompt.contains("id, amount"));
        assert!(prompt.contains("空值"));
    }

    #[test]
    fn data_analysis_prompt_contains_params() {
        let prompt = data_analysis_prompt("SELECT * FROM sales", 5, "region, revenue\nA, 500");
        assert!(prompt.contains("SELECT * FROM sales"));
        assert!(prompt.contains("5"));
        assert!(prompt.contains("region, revenue"));
        assert!(prompt.contains("统计特征"));
    }

    #[test]
    fn test_connection_prompt_format() {
        let msgs = test_connection_prompt();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
        assert!(msgs[0].content.contains("连接成功"));
    }
}
