# 拖拽插入 SQL 编辑器时智能补空格

## 背景

从侧边栏拖拽树节点（表、视图、函数等）到 SQL 编辑器时，当前实现（`apps/desktop/src/app.rs` 的 `node_drag_source` 释放逻辑）把内容原样插入到光标位置，不做任何空格处理。例如把 `users` 拖到 `FROM` 之后会得到 `FROMusers`，拖到 `JOIN users` 之后会得到 `JOINusers`，产生粘连。

## 目标

拖拽插入时，根据光标左右相邻字符智能决定是否补空格，避免标识符粘连，同时不在定界符/引号旁引入多余空格。

## 规则

判断只依赖光标左右**相邻的一个字符**，不解析 SQL 语义。

### 左邻字符（光标前一个字符）

若存在且「需要分隔」，则在插入内容前补一个空格。

### 右邻字符（光标处的字符）

若存在且「需要分隔」，则在插入内容后补一个空格。

### 「需要分隔」的判定

单个字符 `c`，以下任一情况**不需要**补空格：

- 空白字符（空格、制表符、换行、回车等）
- 定界符：`(` `)` `,` `.` `;`
- 引号：`'` `"` `` ` ``

其余情况（标识符字符、操作符、其它符号）**需要**补空格。

等价地，`needs_space_padding(c) = !(c.is_whitespace() || matches!(c, '(' | ')' | ',' | '.' | ';' | '\'' | '"' | '`'))`。

左右两侧独立判断，各自成立各自补。

## 实现

### 辅助函数

在 `apps/desktop/src/autocomplete.rs` 中新增：

```rust
/// 拖拽插入 SQL 时，判断光标相邻字符是否需要补空格分隔。
fn needs_space_padding(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '(' | ')' | ',' | '.' | ';' | '\'' | '"' | '`'))
}
```

放在该文件已有的 `is_identifier_char`（第 16 行）附近，同属 SQL 文本处理工具。

### 调用点

修改 `apps/desktop/src/app.rs` 树节点拖拽释放插入逻辑（约 19755 行）：

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
    let new_char_idx = char_idx + insert_text.chars().count();
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

关键点：

- 光标仍放在**插入内容本身之后**，不含后置空格，方便继续输入。
- 插入后仍调用 `autocomplete.dismiss()` 并清空 `last_keystroke`，保持上一轮"拖拽不触发智能提示"的修复。
- `needs_space_padding` 需要导入到 `app.rs`（`use crate::autocomplete::...` 已在第 28-30 行）。

### 边界

- 光标在文本开头：无左邻，前不补空格。
- 光标在文本末尾：无右邻，后不补空格。
- 空 SQL：左右都无字符，原样插入。
- 字节索引与字符索引：`byte_idx` 由 `char_indices` 求得，`tab.sql[..byte_idx]` 与 `tab.sql[byte_idx..]` 都是合法切片边界，不会 panic。

## 测试

在 `apps/desktop/src/autocomplete.rs` 的 `#[cfg(test)]` 模块中新增对 `needs_space_padding` 的单元测试，覆盖：

- 标识符字符 → 需要补空格（`'a'`、`'Z'`、`'0'`、`'_'`）
- 空白 → 不补（`' '`、`'\n'`、`'\t'`）
- 定界符 → 不补（`'('`、`')'`、`','`、`'.'`、`';'`）
- 引号 → 不补（`'\''`、`'"'`、`` '`' ``）
- 操作符 → 补（`'='`、`'>'`、`'+'`）

## 参考

- 拖拽释放插入：`apps/desktop/src/app.rs` 约 19736-19775 行。
- 现有 `is_identifier_char`：`apps/desktop/src/autocomplete.rs` 第 16-18 行。
