# 表信息页右侧 DDL 面板

日期：2026-08-01

## 背景

表信息页（`TableSummaryTab`）目前"查看表定义"的入口通过右键菜单触发，实际行为是打开一个独立的**表标签页**并切换到定义视图（`SummaryContextAction::OpenTable { load_data: false }`）。这不符合"快速查看定义"的直觉——需要跳转离开当前表信息页。

目标：在表信息页内新增一个可折叠的右侧面板，用于就地展示选中对象（表/视图/存储过程/函数）的 DDL。提供图标按钮（`折叠导航_o.svg`）切换面板展开/收起，右键菜单"查看表定义"改为打开该面板。

## 设计

### 1. 布局结构

表信息页内容区由单栏改为**左右两栏**（复用查询页 `saved_queries_panel` 的"面板 + 拖拽条"模式，见 app.rs:10455）：

```
┌────────────────────────────────────────────────────────────┐
│ 工具栏：[定位][标题] [表|视图|存储过程|函数]  [搜索框]  [新图标] │  ← 38px
├──────────────────────────────┬─────────────────────────────┤
│                              │ 拖拽条 │  DDL 面板           │
│        表格（现有）           │  │     │  对象名 + 复制按钮   │
│                              │        │  格式化 DDL          │
├──────────────────────────────┴────────┴────────────────────┤
│ 状态栏：共 N 个表                   已选 M 个                │  ← 26px
└────────────────────────────────────────────────────────────┘
```

- 面板位于**表格右侧**，宽度可拖拽（150–1000px，默认 320px），折叠后保留上次宽度
- 面板折叠时内容区回到现有单栏布局
- 工具栏新增图标按钮（仿 `locate_icon_button` 实现，见 app.rs:32470，`egui::include_image!` 加载 SVG 并 tint 上色），点击切换面板展开/收起

### 2. 状态字段（`TableSummaryTabState` 新增）

```rust
ddl_panel_visible: bool,     // 面板开关
ddl_panel_width: f32,        // 面板宽度（拖拽调整）
ddl_resize_drag: Option<f32>,
ddl_target: Option<DdlTarget>,      // 当前展示对象
ddl_text: Option<String>,           // 已加载的 DDL（成功）
ddl_error: Option<String>,          // 加载失败信息
ddl_loading: bool,                  // 异步加载中
```

```rust
enum DdlTarget {
    Table(ExplorerNode),       // 表/视图/集合
    Routine(ExplorerNode),     // 存储过程/函数
}
```

### 3. 交互流程

- **右键菜单"查看表定义/视图/存储过程/函数定义"**（表信息页 + 侧边栏两处）→ 打开对应表信息页 Tab、设置 `ddl_target`、展开面板、异步加载 DDL。原"打开定义视图标签页"行为被替换
- **图标点击** → 切换面板；无选中行时显示占位提示"请先在表格中选择一个对象"
- **单击/右键选中行** → 面板跟随更新（展示最后交互的行）
- **异步加载**：仿照现有 `pending_table_summary` 模式（app.rs:2925 轮询），`load_table_definition` / `load_routine_definition` 后台线程 → channel 回传

### 4. 复用与文本

- DDL 渲染复用 `render_definition_sql_view`（app.rs:25369，格式化高亮 + 复制按钮 + 编辑器配色，已在表/视图/Routine 页使用）
- 所有新增用户可见文本用 `tr!` 包裹，并在 `crates/i18n/src/lib.rs` 的 `en()` 补英文翻译

### 5. 范围裁剪

- MongoDB 集合定义（`load_table_definition` 已有 `create_sql` 支持，driver-mongodb src/lib.rs:255）同样可展示，不额外开发
- 侧边栏右键"查看表定义"改为：打开对应表信息页 Tab + 展开面板。若目标表信息页尚未打开则自动创建（复用 `OpenTableSummary` 逻辑）；`OpenTableSummary` 新增目标对象参数

## 涉及文件

- `apps/desktop/src/app.rs` — 主要改动（状态字段、布局、右键菜单、动作处理、异步加载）
- `apps/desktop/assets/svg/layout.svg` — 新增图标（源自用户提供的 `折叠导航_o.svg`，改为 tint 适用的填充色）
- `crates/i18n/src/lib.rs` — 补充英文翻译

## 不做的事

- 不改变侧边栏对象树其他右键行为（"在新标签页中打开"仍打开数据页）
- 不做全局主窗口级固定 DDL 面板
- 不重构表信息页现有的表格渲染逻辑
