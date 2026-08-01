# 拖拽插入 SQL 编辑器智能补空格 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 侧边栏拖拽表/视图/函数等到 SQL 编辑器时，根据光标左右相邻字符智能补空格，避免标识符粘连。

**Architecture:** 在 `autocomplete.rs` 新增 `pub(crate) fn needs_space_padding(c: char) -> bool` 判断单字符是否需要补空格分隔；在 `app.rs` 树节点拖拽释放插入点（约 19755 行）调用它，对左邻/右邻字符独立判断，前后补空格。光标仍放在插入内容之后（不含后置空格）。

**Tech Stack:** Rust / egui / freedb desktop crate

## Global Constraints

- 所有产出物（注释、文档、任务描述）使用简体中文。
- 所有 UI 文本必须用 `tr!` 宏包裹（本次改动不涉及新增 UI 文本）。
- 简洁优先，不添加要求之外的功能。
- 精准修改，只碰必须碰的代码。
- 规则只依赖光标左右**相邻的一个字符**，不解析 SQL 语义。
- 插入后保持 `autocomplete.dismiss()` + 清空 `last_keystroke`（防拖拽触发智能提示的既有修复）。
- 光标位置 = 插入内容之后，不含后置空格。

---

### Task 1: 新增 `needs_space_padding` 辅助函数及单元测试

**Files:**
- Modify: `apps/desktop/src/autocomplete.rs:16-18`（`is_identifier_char` 之后新增函数）
- Test: `apps/desktop/src/autocomplete.rs:2260`（`#[cfg(test)] mod tests` 内新增测试）

**Interfaces:**
- Consumes: 无（独立工具函数）
- Produces: `pub(crate) fn needs_space_padding(c: char) -> bool` — 供 Task 2 的 `app.rs` 使用

- [ ] **Step 1: 编写失败测试**

在 `apps/desktop/src/autocomplete.rs` 的 `#[cfg(test)] mod tests`（第 2260 行起）末尾追加：

```rust
    #[test]
    fn needs_space_padding_identifier_chars() {
        assert!(needs_space_padding('a'));
        assert!(needs_space_padding('Z'));
        assert!(needs_space_padding('0'));
        assert!(needs_space_padding('_'));
    }

    #[test]
    fn needs_space_padding_whitespace() {
        assert!(!needs_space_padding(' '));
        assert!(!needs_space_padding('\n'));
        assert!(!needs_space_padding('\t'));
        assert!(!needs_space_padding('\r'));
    }

    #[test]
    fn needs_space_padding_delimiters() {
        assert!(!needs_space_padding('('));
        assert!(!needs_space_padding(')'));
        assert!(!needs_space_padding(','));
        assert!(!needs_space_padding('.'));
        assert!(!needs_space_padding(';'));
    }

    #[test]
    fn needs_space_padding_quotes() {
        assert!(!needs_space_padding('\''));
        assert!(!needs_space_padding('"'));
        assert!(!needs_space_padding('`'));
    }

    #[test]
    fn needs_space_padding_operators() {
        assert!(needs_space_padding('='));
        assert!(needs_space_padding('>'));
        assert!(needs_space_padding('+'));
        assert!(needs_space_padding('-'));
        assert!(needs_space_padding('*'));
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p desktop --bin freedb needs_space_padding 2>&1 | tail -20`

Expected: 编译报错 `cannot find function 'needs_space_padding' in this scope`，测试失败。

- [ ] **Step 3: 编写最小实现**

在 `apps/desktop/src/autocomplete.rs` 的 `is_identifier_char`（第 16-18 行）之后新增：

```rust
/// 拖拽插入 SQL 时，判断光标相邻字符是否需要补空格分隔。
pub(crate) fn needs_space_padding(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '(' | ')' | ',' | '.' | ';' | '\'' | '"' | '`'))
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p desktop --bin freedb needs_space_padding 2>&1 | tail -20`

Expected: 5 个测试全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add apps/desktop/src/autocomplete.rs
git commit -m "feat(editor): 新增拖拽插入空格判断辅助函数 needs_space_padding"
```

---

### Task 2: 拖拽释放插入时应用空格补齐

**Files:**
- Modify: `apps/desktop/src/app.rs:28-32`（import）
- Modify: `apps/desktop/src/app.rs:19754-19775`（拖拽释放插入逻辑）

**Interfaces:**
- Consumes: `pub(crate) fn needs_space_padding(c: char) -> bool`（Task 1 定义）
- Produces: 无（行为修改）

- [ ] **Step 1: 更新 import**

在 `apps/desktop/src/app.rs` 第 28-32 行的 `use crate::autocomplete::{ ... }` 中追加 `needs_space_padding`：

```rust
use crate::autocomplete::{
    apply_autocomplete_suggestion, autocomplete_min_prefix_len, autocomplete_palette, needs_space_padding, render_autocomplete_popup, AutocompleteEngine, AutocompletePalette,
    AutocompleteState, AutocompleteSuggestion, AutocompleteUsageMemory, SchemaCache, SqlContext, SqlContextParser,
    SuggestionKind,
};
```

- [ ] **Step 2: 修改拖拽释放插入逻辑**

将 `apps/desktop/src/app.rs` 第 19755-19775 行替换为：

```rust
if let Some(WorkspaceTab::Query(tab)) = self.tabs.get_mut(self.active_tab) {
    let char_idx = tab.cursor_range
        .map(|r| r.primary.index)
        .unwrap_or(tab.sql.chars().count());
    // char_idx 是字符索引，需要转成字节索引
    let byte_idx = tab.sql.char_indices()
        .nth(char_idx)
        .map(|(pos, _)| pos)
        .unwrap_or(tab.sql.len());

    // 前后空格补齐（仅依据光标相邻字符）
    let left = tab.sql[..byte_idx].chars().next_back();
    let right = tab.sql[byte_idx..].chars().next();
    let pad_left = left.is_some_and(needs_space_padding);
    let pad_right = right.is_some_and(needs_space_padding);
    let padded = format!(
        "{}{}{}",
        if pad_left { " " } else { "" },
        insert_text,
        if pad_right { " " } else { "" },
    );

    tab.sql.insert_str(byte_idx, &padded);
    // 光标放在插入内容之后（不含后置空格），需计入前置补的空格
    let new_char_idx = char_idx + usize::from(pad_left) + insert_text.chars().count();
    tab.cursor_range = Some(egui::text::CCursorRange::one(
        egui::text::CCursor::new(new_char_idx),
    ));
    // 拖拽插入内容时不触发智能提示
    tab.autocomplete.dismiss();
    tab.autocomplete.last_keystroke = None;
    // 请求焦点并设置光标到插入内容之后
    tab.editor_focus_requested = true;
    tab.autocomplete_cursor_target = Some(new_char_idx);
}
```

- [ ] **Step 3: 编译验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check -p desktop 2>&1 | grep -E "^error|Finished"`

Expected: `Finished`，无新 error（既有 235 个 warning 与本次改动无关）。

- [ ] **Step 4: 运行全部单元测试**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p desktop --bin freedb 2>&1 | tail -15`

Expected: 全部测试 PASS（含 Task 1 的 5 个 `needs_space_padding` 测试）。

- [ ] **Step 5: 提交**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(editor): 拖拽插入 SQL 时按相邻字符智能补空格"
```

---

### Task 3: 构建并重启验证

**Files:**
- 无代码修改

**Interfaces:**
- Consumes: Task 1、Task 2 的产出
- Produces: 可运行的 freedb 新构建

- [ ] **Step 1: 构建 debug 版本**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build --package desktop --bin freedb 2>&1 | tail -3`

Expected: `Finished`，无 error。

- [ ] **Step 2: 重启 freedb**

Run: `pkill -9 -f "target/debug/freedb" 2>/dev/null; sleep 1; /Users/fdr/aiProjects/freedb/target/debug/freedb & sleep 2 && pgrep -f "target/debug/freedb" && echo "freedb 已启动"`

Expected: 输出 `freedb 已启动`。

- [ ] **Step 3: 手动验证（需用户操作）**

请用户验证以下场景：

| 场景 | 操作 | 期望结果 |
|---|---|---|
| 光标在 `FROM` 后无空格 | 拖入 `users` | `FROM users` |
| 光标在 `FROM␣` 后 | 拖入 `users` | `FROM␣users`（前不补） |
| 光标在 `JOIN␣users` 后 | 拖入 `orders` | `JOIN␣users␣orders`（左邻 `s` 补后空格） |
| 光标在 `WHERE␣id=` 后 | 拖入 `status` | `WHERE␣id= status`（左邻 `=` 补前空格） |
| 光标在 `db.` 后 | 拖入 MongoDB collection | `db.collection`（左邻 `.` 不补） |
| 光标在空 SQL | 拖入 `users` | `users` |
| 光标在 `'id'` 引号内 | 拖入内容 | 引号旁不补空格 |

Expected: 所有场景符合预期，且不弹出智能提示。
