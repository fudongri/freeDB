# 数据页/查询结果页全选行

## 背景与目标

数据页和查询结果页当前只能通过逐行点击（配合 Shift/Cmd）选择多行。当需要复制、删除大量数据时操作繁琐。本设计为三个表格视图增加「全选行」能力：

- 数据页（`render_table_tab`）
- 普通查询结果页（`render_result_table`）
- 可编辑查询结果页（简单单表 SELECT 时，`render_editable_result_table`）

两种触发方式：点击表头 `#`、快捷键 `cmd+a`。

## 交互语义

| 触发 | 行为 |
|------|------|
| 点击 `#` 表头 | **切换**：若已全选（选中行数 == 总行数）→ 清除全部行选择；否则 → 全选所有行 |
| `cmd+a` | **全选**：无矩形选区时全选所有行；矩形选区激活时保持现状（全选单元格） |
| 两者 | 全选时清除列选中和矩形选区（与现有「选行时清除列选中」行为一致） |

### cmd+a 协调

数据页（app.rs:23427）和可编辑查询结果页（app.rs:22217）已有「矩形选区激活时 cmd+a 全选单元格」逻辑，均位于 `if cell_selection_anchor.is_some()` 块内。新增的全选行检测放在该块**之外**，两者天然协调互不干扰。普通查询结果页无现有 cmd+a 处理，无冲突。

### 表格区域限定

egui 的 TextEdit（SQL 编辑器、筛选框、搜索框）在自身聚焦时会消费 `cmd+a` 做全选文本，表格渲染函数收不到该按键——天然实现「编辑框优先，表格区域全选行」，无需额外焦点判断。

## 实现

全部改动在单文件 `apps/desktop/src/app.rs`。

### 1. `table_header_cell`（28162 行）

将

```rust
if sortable && cell_response.clicked() {
```

改为

```rust
if cell_response.clicked() {
```

让非 sortable 表头（`#`）也能返回点击。函数内部已用 `ui.interact` 捕获点击；`sortable` 参数只影响排序语义，不影响其它行为。所有非 sortable 调用点（建表页、表结构页等）均以 `let (_, _, _, _)` 忽略返回值，无副作用。

### 2. 数据页 `render_table_tab`（22869 行）

`#` 表头改为接收 `clicked`，做 toggle 全选：

```rust
let (_, clicked, _, _) = table_header_cell(ui, &palette, "#", false, None, false, false, None, false, false);
if clicked {
    select_all_preview_rows(tab, row_count);
    tab.selected_columns.clear();
    clear_table_cell_selection(tab);
    ui.ctx().request_repaint();
}
```

在矩形选区键盘处理块（23416 行）**外**新增 cmd+a 检测。注意 pending insert 模式（`tab.pending_insert_row.is_some()`）时行号列隐藏、无法点击 `#`，cmd+a 也应跳过该模式保持一致：

```rust
if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
    if tab.cell_selection_anchor.is_none() && tab.pending_insert_row.is_none() {
        let row_count = tab.preview.as_ref().map(|p| p.rows.len()).unwrap_or(0);
        select_all_preview_rows(tab, row_count);
        tab.selected_columns.clear();
        clear_table_cell_selection(tab);
    }
}
```

### 3. 普通查询结果页 `render_result_table`（21137 行）

`#` 表头接收 `clicked` 做 toggle：

```rust
let (_, clicked, _, _) = table_header_cell(ui, &palette, "#", false, None, false, false, None, false, false);
if clicked {
    select_all_query_rows(selected_rows, selected_row, selection_anchor, result.rows.len());
    selected_columns.clear();
    ui.ctx().request_repaint();
}
```

在 `#` 表头块内（toggle 全选行逻辑之后）新增 cmd+a 检测，与 toggle 逻辑集中、可访问全部所需引用：

```rust
if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
    if cell_selection_anchor.is_none() {
        select_all_query_rows(selected_rows, selected_row, selection_anchor, result.rows.len());
        selected_columns.clear();
    }
}
```

### 4. 可编辑查询结果页 `render_editable_result_table`（21828 行）

`#` 表头接收 `clicked` 做 toggle（同上）。

cmd+a：现有矩形选区全选单元格逻辑（22217 行）保留，在其**外**新增无矩形选区时全选行。

### 辅助函数

```rust
// 数据页：全选所有行
fn select_all_preview_rows(tab: &mut TableTabState, row_count: usize) {
    if tab.selected_preview_rows.len() == row_count {
        tab.selected_preview_rows.clear();
        tab.selected_preview_row = None;
        tab.selection_anchor_row = None;
    } else {
        tab.selected_preview_rows.clear();
        for i in 0..row_count {
            tab.selected_preview_rows.insert(i);
        }
        tab.selected_preview_row = row_count.checked_sub(1);
        tab.selection_anchor_row = Some(0);
    }
    normalize_preview_selection(tab);
}

// 查询页：全选所有行
fn select_all_query_rows(
    selected_rows: &mut BTreeSet<usize>,
    selected_row: &mut Option<usize>,
    anchor: &mut Option<usize>,
    row_count: usize,
) {
    if selected_rows.len() == row_count {
        selected_rows.clear();
        *selected_row = None;
        *anchor = None;
    } else {
        selected_rows.clear();
        for i in 0..row_count {
            selected_rows.insert(i);
        }
        *selected_row = row_count.checked_sub(1);
        *anchor = Some(0);
    }
    normalize_query_selection(selected_rows, selected_row, anchor);
}
```

全选时设 `selected_row = row_count - 1`、`anchor = 0`，保证后续 shift 扩展语义正常。

## 性能

全选是 O(n) 插入到 `BTreeSet`，万行数据约毫秒级；toggle 判定（`len == row_count`）O(1)。数据页上万行场景可接受，无需上限。

## 测试

构建 + 手动验证三处：

1. 数据页：点 `#` 全选，再点取消；右键菜单显示「复制选中 N 条数据」「删除选中 N 条记录」
2. 查询结果页：`cmd+a` 全选行；编辑框（SQL 编辑器）聚焦时 `cmd+a` 全选文本不触发全选行
3. 矩形选区激活时 `cmd+a` 仍全选单元格
4. 全选后 shift 点击某行，范围选择行为正常
