# 表信息页右侧 DDL 面板 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在表信息页（`TableSummaryTab`）内新增可折叠的右侧 DDL 面板，就地展示选中对象（表/视图/存储过程/函数）的 DDL；工具栏图标切换展开/收起；表信息页与侧边栏右键菜单的"查看定义"改为打开该面板。

**Architecture:** 表信息页内容区由单栏改为左右两栏：左侧保留现有表格渲染（复用查询页 `saved_queries_panel` 的拖拽条模式），右侧新增 DDL 面板。面板数据通过异步 channel 加载（复用 `pending_table_summary` / `pending_routine_definition` 轮询模式）。DDL 渲染复用现成的 `render_definition_sql_view`。侧边栏右键通过扩展现有 `SidebarAction::OpenTableSummary` 变体携带目标对象，自动打开/定位表信息页并设置面板目标。

**Tech Stack:** Rust / egui 0.33 / egui_extras StripBuilder + TableBuilder / eframe / tokio

## Global Constraints

- 所有用户可见文本必须用 `tr!("中文")` 包裹，并在 `crates/i18n/src/lib.rs` 的 `en()` 中补英文翻译（match key 与中文字符串完全一致）
- 面板/工具栏按钮禁止 `ui.button()` 默认样式，必须用 `toolbar_button()` / `mini_button()`（见 CLAUDE.md 第 6 节）
- 面板默认展开宽度 320px，拖拽范围 150–500px
- 目标文件：`apps/desktop/src/app.rs`（唯一 UI 文件）、`apps/desktop/assets/svg/layout.svg`（新增）、`crates/i18n/src/lib.rs`（翻译）
- 每个 Task 结束需 `cargo build`（或 `cargo check`）验证编译通过

---

### Task 1: 新增 SVG 图标资产

**Files:**
- Create: `apps/desktop/assets/svg/layout.svg`

**Interfaces:**
- Produces: `apps/desktop/assets/svg/layout.svg` — 供 Task 2 用 `egui::include_image!("../assets/svg/layout.svg")` 加载

- [ ] **Step 1: 创建 SVG 文件**

用户提供的 `/Users/fdr/Downloads/折叠导航_o.svg` 原始 path 填充色是 `#444444`，但 `locate.svg` 用白色填充 + egui `.tint()` 上色。为了配合现有模式，将 path 的 `fill` 改为白色（与 `collapse-all.svg`、`locate.svg` 一致），保留原始 path 几何。

创建 `apps/desktop/assets/svg/layout.svg`：

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="200" height="200"><path fill="#ffffff" d="M341.333333 298.666667H213.333333v469.333333h640V298.666667H384v469.333333H341.333333V298.666667z m554.666667-42.666667v554.666667H170.666667V256h725.333333z" p-id="5297"></path></svg>
```

验证：`cat apps/desktop/assets/svg/layout.svg` 应显示上述内容（单行 SVG）。

- [ ] **Step 2: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS（仅新增资源文件，不涉及代码）

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/assets/svg/layout.svg
git commit -m "assets(summary): 新增 DDL 面板折叠导航图标"
```

---

### Task 2: 表信息页状态字段与 DdlTarget 类型

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes: `core_domain::ExplorerNode`（已有）、`ExplorerNodeType`（已有）
- Produces:
  - `enum DdlTarget { Table(ExplorerNode), Routine(ExplorerNode) }` — 面板当前展示目标
  - `TableSummaryTabState` 新增字段（见下方代码块）
  - `SummaryContextAction::ShowDdl { node: ExplorerNode }` — 新的右键菜单动作
  - `struct DdlLoadResult` — 异步加载结果

- [ ] **Step 1: 新增 DdlTarget 枚举与 DdlLoadResult**

在 `struct TableSummaryLoadResult` 定义（约 app.rs:297）附近新增：

```rust
/// 表信息页 DDL 面板异步加载结果
struct DdlLoadResult {
    tab_id: String,
    result: Result<String, String>,
}
```

在 `SummaryFilter` 枚举定义（约 app.rs:315）附近新增：

```rust
/// DDL 面板当前展示的目标对象
#[derive(Clone)]
enum DdlTarget {
    Table(ExplorerNode),
    Routine(ExplorerNode),
}
```

- [ ] **Step 2: 扩展 `TableSummaryTabState`**

在 `TableSummaryTabState` 结构（app.rs:319-356）中，`cached_summary_filter` 字段后新增：

```rust
    // 右侧 DDL 面板状态
    ddl_panel_visible: bool,
    ddl_panel_width: f32,
    ddl_loading: bool,
    ddl_target: Option<DdlTarget>,
    ddl_text: Option<String>,
    ddl_error: Option<String>,
```

- [ ] **Step 3: 扩展 `SummaryContextAction`**

在 `SummaryContextAction` 枚举（app.rs:383）的 `Reload` 变体附近新增：

```rust
    ShowDdl { node: ExplorerNode },
```

- [ ] **Step 4: 扩展 app 结构体的异步接收器字段**

在 app 结构体（约 app.rs:150）的 `pending_routine_definition` 字段后新增：

```rust
    pending_ddl_load: HashMap<String, Receiver<DdlLoadResult>>,
```

在 `App` 初始化（约 app.rs:1773，`pending_routine_definition: HashMap::new(),` 之后）新增：

```rust
            pending_ddl_load: HashMap::new(),
```

- [ ] **Step 5: 更新所有 `TableSummaryTabState` 构造点**

搜索 `cached_summary_filter:`（app.rs:6263 附近是初始构造点），在每处 `cached_summary_filter: SummaryFilter::Tables,` 后新增：

```rust
                        ddl_panel_visible: false,
                        ddl_panel_width: 320.0,
                        ddl_loading: false,
                        ddl_target: None,
                        ddl_text: None,
                        ddl_error: None,
```

同时新增的另一处 `TableSummaryTabState` 构造（app.rs:7854 附近，用于新建表信息页）也要补上（`pending_open_table: None,` 附近的结构体字面量）。

- [ ] **Step 6: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS（新字段均为字面量初始化，无未初始化错误）

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): DDL 面板状态字段与 DdlTarget 类型"
```

---

### Task 3: DDL 面板内容渲染函数

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - `render_definition_sql_view(ui, title, create_sql, colors, fonts)`（app.rs:25369，已有，复用）
  - `DdlTarget`（Task 2）
  - `tab.ddl_loading` / `tab.ddl_text` / `tab.ddl_error` / `tab.ddl_target`（Task 2）
- Produces: `fn render_summary_ddl_panel(ui, tab: &TableSummaryTabState, colors, fonts, panel_width: f32)` — 供 Task 4 在右侧面板中调用

- [ ] **Step 1: 新增 `render_summary_ddl_panel` 函数**

在 `render_definition_sql_view`（app.rs:25369）之前新增一个独立函数。该函数接收只读 `&TableSummaryTabState`，根据加载状态渲染面板内容：

```rust
/// 表信息页右侧 DDL 面板内容：对象名 + 复制按钮 + 格式化 DDL（或加载/错误/占位状态）
fn render_summary_ddl_panel(
    ui: &mut egui::Ui,
    tab: &TableSummaryTabState,
    colors: &ui_theme::ThemeColors,
    fonts: &ui_theme::FontSizes,
) {
    let palette = mac_ui_palette_from_ui(ui);
    if let Some(ddl_text) = &tab.ddl_text {
        let title = match &tab.ddl_target {
            Some(DdlTarget::Table(node)) | Some(DdlTarget::Routine(node)) => node.name.clone(),
            None => String::new(),
        };
        render_definition_sql_view(ui, &title, ddl_text, colors, fonts);
    } else if tab.ddl_loading {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.add(egui::Spinner::new().size(fonts.spinner_size));
            ui.add_space(8.0);
            ui.label(RichText::new(tr!("正在加载定义...")).color(palette.weak_text));
        });
    } else if let Some(error) = &tab.ddl_error {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new(format!("{}: {}", tr!("加载失败"), error)).color(palette.danger));
        });
    } else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new(tr!("请先在表格中选择一个对象")).color(palette.weak_text));
        });
    }
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

错误色字段已确认：`MacUiPalette` 的错误色字段是 `danger`（app.rs:20266），非 `error_text`。`fonts.spinner_size` 已存在（app.rs:12177 同 Tab 已使用）。若编译报错，对照 `MacUiPalette` 实际字段名调整。

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): DDL 面板内容渲染函数"
```

---

### Task 4: 工具栏图标按钮 + 面板展开/收起

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - `apps/desktop/assets/svg/layout.svg`（Task 1）
  - `locate_icon_button` 模式（app.rs:32470，参考实现）
  - `SummaryContextAction::ShowDdl`（Task 2）
  - `tab.pending_open_table`（已有，双击触发）
- Produces:
  - 工具栏图标切换 `tab.ddl_panel_visible`
  - 图标点击时若 `tab.ddl_target.is_none()` 且有选中行 → push `ShowDdl`
  - `fn summary_ddl_toolbar_button(ui, tint) -> egui::Response` — 与 `locate_icon_button` 一致，返回 Response 以便链 `.on_hover_text().clicked()`

- [ ] **Step 1: 新增 `summary_ddl_toolbar_button` 函数**

在 `locate_icon_button`（app.rs:32470）附近新增一个相似函数（返回 `egui::Response`，与 `locate_icon_button` 签名一致）：

```rust
/// 表信息页 DDL 面板开关图标按钮：白色 SVG tint 成标题色，hover 显示浅底，与侧边栏定位按钮一致
fn summary_ddl_toolbar_button(ui: &mut egui::Ui, tint: egui::Color32) -> egui::Response {
    let size = egui::vec2(26.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, ui.visuals().widgets.hovered.corner_radius, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        egui::Image::new(egui::include_image!("../assets/svg/layout.svg"))
            .fit_to_exact_size(egui::vec2(18.0, 18.0))
            .tint(tint)
            .paint_at(ui, egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, 1.0), egui::vec2(18.0, 18.0)));
    }
    response
}
```

- [ ] **Step 2: 在工具栏添加图标按钮**

在 `render_table_summary_tab` 的工具栏（app.rs:12082 附近，`locate_icon_button` 调用之后）新增：

```rust
                                // DDL 面板开关
                                if summary_ddl_toolbar_button(ui, palette.text)
                                    .on_hover_text(tr!("查看 DDL 面板"))
                                    .clicked()
                                {
                                    tab.ddl_panel_visible = !tab.ddl_panel_visible;
                                    // 展开时若有选中行但尚无目标，自动加载选中行的 DDL
                                    if tab.ddl_panel_visible && tab.ddl_target.is_none() {
                                        if let Some(&ri) = tab.selected_indices.iter().next() {
                                            if let Some(s) = Self::summary_display_items(tab).get(ri) {
                                                let node = summary_to_node(s, tab);
                                                tab.pending_actions.push(SummaryContextAction::ShowDdl { node });
                                            }
                                        }
                                    }
                                }
```

- [ ] **Step 3: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): 工具栏 DDL 面板开关图标"
```

---

### Task 5: 内容区左右两栏布局 + 拖拽条

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - `render_summary_ddl_panel`（Task 3）
  - `tab.ddl_panel_visible` / `tab.ddl_panel_width`（Task 2）
  - 现有表格渲染块（app.rs:12175-12760，ScrollArea + TableBuilder）
  - 查询页拖拽条模式（app.rs:10455-10560，参考实现）
- Produces: 面板展开时的左右两栏布局

- [ ] **Step 1: 在内容区渲染 DDL 面板**

在 `render_table_summary_tab` 的内容区 cell（app.rs:12168，`strip.cell(|ui| {` 且 `egui::Frame` fill 为 `palette.card_bg` 的那个）中，在 `if Self::summary_display_items(tab).is_empty() { ... return; }` 块之后、`egui::ScrollArea::horizontal()` 之前，插入：

```rust
                            // 右侧 DDL 面板（展开时）
                            if tab.ddl_panel_visible {
                                egui::SidePanel::right(format!("ddl_panel_{}", tab.id))
                                    .resizable(true)
                                    .default_width(tab.ddl_panel_width)
                                    .min_width(150.0)
                                    .max_width(500.0)
                                    .show_inside(ui, |ui| {
                                        ui.set_min_height(ui.available_height());
                                        render_summary_ddl_panel(ui, tab, colors, fonts);
                                    });
                            }
```

`SidePanel::show_inside` 会自动将父 ui 的可用区域让给面板并收缩后续内容（参考 egui-0.33.3 panel.rs `show_inside_dyn`：`ui.set_cursor` 根据面板 rect 收缩），因此表格的 `ScrollArea` 代码**无需改动**，会自然占满面板左侧的剩余空间。`resizable(true)` 内置拖拽调宽（150–500px 由 `min_width`/`max_width` 限制），拖拽宽度由 egui 的 `PanelState` 按 `id` 持久化，无需手动存 `ddl_panel_width` 到字段（`ddl_panel_width` 字段仅作为初始默认宽度 320.0）。

- [ ] **Step 2: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

注意：若编译报 `borrow of moved value`（`SidePanel` 闭包捕获 `tab` 后表格又借用 `tab`），在 `SidePanel::show_inside` 的闭包中只做只读借用（`render_summary_ddl_panel` 已只读 `tab`），表格渲染仍在闭包外使用 `&mut tab`，两者在帧内的顺序借用应无冲突。若仍冲突，将 `show_inside` 的闭包改为仅读取 `tab.ddl_*` 字段到局部变量，再在闭包外用这些局部变量渲染。

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): 内容区右侧 DDL 面板布局与拖拽条"
```

---

### Task 6: 右键菜单动作接线（表信息页）

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - `SummaryContextAction::ShowDdl`（Task 2）
  - `summary_to_node`（app.rs:25145，已有）
  - `TableRef` / `RoutineRef`（core_domain，已有）
  - `self.services.load_table_definition(&TableRef)` / `load_routine_definition(&RoutineRef)`（app_services，已有）
- Produces:
  - 表信息页右键"查看表定义"等改为 push `ShowDdl`
  - `SummaryContextAction::ShowDdl` 的处理分支（异步加载 + 写回 tab.ddl_*）

- [ ] **Step 1: 修改表信息页右键菜单**

在 `render_table_summary_tab` 的右键菜单（app.rs:12471 附近，`let def_label = ...` 之后的 `if ui.button(def_label).clicked()` 分支）：

现有：
```rust
                                                            let def_label = if is_view {
                                                                tr!("查看视图定义")
                                                            } else if is_routine && row_type == "PROCEDURE" {
                                                                tr!("查看存储过程定义")
                                                            } else if is_routine {
                                                                tr!("查看函数定义")
                                                            } else if kind == DatabaseKind::MongoDb {
                                                                tr!("查看集合定义")
                                                            } else {
                                                                tr!("查看表定义")
                                                            };
                                                            if ui.button(def_label).clicked() {
                                                                tab.pending_actions.push(SummaryContextAction::OpenTable {
                                                                    node: node.clone(),
                                                                    load_data: false,
                                                                    force_new_tab: false,
                                                                });
                                                                ui.close();
                                                            }
```

改为：
```rust
                                                            let def_label = if is_view {
                                                                tr!("查看视图定义")
                                                            } else if is_routine && row_type == "PROCEDURE" {
                                                                tr!("查看存储过程定义")
                                                            } else if is_routine {
                                                                tr!("查看函数定义")
                                                            } else if kind == DatabaseKind::MongoDb {
                                                                tr!("查看集合定义")
                                                            } else {
                                                                tr!("查看表定义")
                                                            };
                                                            if ui.button(def_label).clicked() {
                                                                tab.pending_actions.push(SummaryContextAction::ShowDdl {
                                                                    node: node.clone(),
                                                                });
                                                                ui.close();
                                                            }
```

- [ ] **Step 2: 处理 `ShowDdl` 动作**

在 `SummaryContextAction` 的 `match` 处理（app.rs:7650 附近，`OpenTable` 分支之后）新增：

```rust
                            SummaryContextAction::ShowDdl { node } => {
                                let is_routine = matches!(node.node_type, ExplorerNodeType::Procedure | ExplorerNodeType::Function);
                                if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.get_mut(self.active_tab) {
                                    tab.ddl_panel_visible = true;
                                    tab.ddl_loading = true;
                                    tab.ddl_text = None;
                                    tab.ddl_error = None;
                                    tab.ddl_target = Some(if is_routine {
                                        DdlTarget::Routine(node.clone())
                                    } else {
                                        DdlTarget::Table(node.clone())
                                    });
                                    let tab_id = tab.id.clone();
                                    let connection_id = node.connection_id.clone();
                                    let database = node.database.clone();
                                    let schema = node.schema.clone();
                                    let name = node.name.clone();
                                    let services = self.services.clone();
                                    let (sender, receiver) = mpsc::channel();
                                    self.pending_ddl_load.insert(tab_id.clone(), receiver);
                                    let is_proc = matches!(node.node_type, ExplorerNodeType::Procedure);
                                    self.runtime.handle().spawn(async move {
                                        let result = if is_routine {
                                            let routine = core_domain::RoutineRef {
                                                connection_id,
                                                database,
                                                schema,
                                                name,
                                                is_procedure: is_proc,
                                            };
                                            services.load_routine_definition(&routine).await.map(|d| d.create_sql.unwrap_or_default())
                                        } else {
                                            let table = TableRef {
                                                connection_id,
                                                database,
                                                schema,
                                                table: name,
                                                is_view: matches!(node.node_type, ExplorerNodeType::View),
                                            };
                                            services.load_table_definition(&table).await.map(|d| d.create_sql.unwrap_or_default())
                                        };
                                        let _ = sender.send(DdlLoadResult {
                                            tab_id,
                                            result: result.map_err(|e| e.to_string()),
                                        });
                                    });
                                }
                            }
```

借用说明：在 `if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.get_mut(...)` 块内，`self.pending_ddl_load.insert(...)`、`self.runtime.handle().clone()` 属于对 `self` 的**不相交字段**访问，与现有 `LoadRoutines` 分支（app.rs:7747）中的 `self.pending_routine_summary = Some(...)` 模式完全一致，可正常编译。不要将加载逻辑抽成 `&mut self` 方法在块内调用——方法调用会整体借用 `self`，与 `tab` 的可变借用冲突。

- [ ] **Step 3: 轮询 DDL 加载结果**

在 `pending_routine_definition` 轮询块（app.rs:3039 附近）之后新增：

```rust
        // Poll DDL panel load results
        let mut finished_ddl = Vec::new();
        for (tab_id, receiver) in &self.pending_ddl_load {
            match receiver.try_recv() {
                Ok(result) => {
                    if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.iter_mut().find(|t| {
                        matches!(t, WorkspaceTab::TableSummary(ts) if ts.id == result.tab_id)
                    }) {
                        tab.ddl_loading = false;
                        match result.result {
                            Ok(text) => tab.ddl_text = Some(text),
                            Err(e) => tab.ddl_error = Some(e),
                        }
                    }
                    finished_ddl.push(tab_id.clone());
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.iter_mut().find(|t| {
                        matches!(t, WorkspaceTab::TableSummary(ts) if ts.id == *tab_id)
                    }) {
                        tab.ddl_loading = false;
                        tab.ddl_error = Some(tr!("加载失败：任务异常终止").to_string());
                    }
                    finished_ddl.push(tab_id.clone());
                }
            }
        }
        for tab_id in finished_ddl {
            self.pending_ddl_load.remove(&tab_id);
        }
```

- [ ] **Step 4: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): 表信息页右键查看定义改为 DDL 面板"
```

---

### Task 7: 侧边栏右键菜单接线 + 表信息页联动

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - `SidebarAction::OpenTableSummary`（app.rs:19869 枚举、app.rs:6158 处理，已有）
  - `SidebarAction::OpenRoutine`（已有）
  - `SidebarAction::OpenTable`（已有）
  - `SummaryContextAction::ShowDdl`（Task 2）
- Produces: 侧边栏"查看表定义"等 → 打开表信息页 + 设置 DDL 目标并展开面板

- [ ] **Step 1: 修改 `SidebarAction::OpenTableSummary` 变体**

在 `enum SidebarAction`（app.rs:19869）中，将：

```rust
    OpenTableSummary {
        connection_id: String,
        database: String,
        schema: Option<String>,
        db_label: String,
    },
```

改为：

```rust
    OpenTableSummary {
        connection_id: String,
        database: String,
        schema: Option<String>,
        db_label: String,
        ddl_target: Option<ExplorerNode>,
    },
```

- [ ] **Step 2: 更新所有 `OpenTableSummary` 构造点**

搜索 `SidebarAction::OpenTableSummary`（app.rs:6627、6717、7017、7025 附近，以及 SchemaContextAction 内联逻辑 app.rs:7819），每处添加 `ddl_target: None,`。SchemaContextAction::OpenTableSummary 的 handle 分支（app.rs:7819-7830）内联的逻辑中，创建的 `SidebarAction`（若有）同样补 `ddl_target: None`。

- [ ] **Step 3: 修改表/视图右键的"查看定义"**

侧边栏表/视图右键（app.rs:6758-6772 附近）：

现有：
```rust
                        if ui.button(def_label).clicked() {
                            actions.push(SidebarAction::OpenTable { node: node.clone(), load_data: false, force_new_tab: false });
                            ui.close();
                        }
```

改为（保持"在新标签页中打开"不变）：
```rust
                        if ui.button(def_label).clicked() {
                            let open_summary = SidebarAction::OpenTableSummary {
                                connection_id: node.connection_id.clone(),
                                database: node.database.clone().unwrap_or_default(),
                                schema: node.schema.clone(),
                                db_label: node.name.clone(),
                                ddl_target: Some(node.clone()),
                            };
                            actions.push(open_summary);
                            ui.close();
                        }
```

注意：`db_label` 原本是数据库/模式名，用于表信息页标题。这里改用对象名会让标题变成"对象名@连接"。查看现有 `OpenTableSummary` 的 `db_label` 用法（app.rs:6230 附近 `tr!("{}@{} 表信息", db_label, conn_name)`），确认 `db_label` 的语义。若担心标题不正确，改为保持 `db_label` 为数据库名：
- `database` 字段此时是 `node.database.unwrap_or_default()`，可将 `db_label: node.database.clone().unwrap_or_default()`。

最终采用：
```rust
                        if ui.button(def_label).clicked() {
                            actions.push(SidebarAction::OpenTableSummary {
                                connection_id: node.connection_id.clone(),
                                database: node.database.clone().unwrap_or_default(),
                                schema: node.schema.clone(),
                                db_label: node.database.clone().unwrap_or_default(),
                                ddl_target: Some(node.clone()),
                            });
                            ui.close();
                        }
```

- [ ] **Step 4: 修改存储过程/函数右键的"查看定义"**

侧边栏 routine 右键（app.rs:6931-6938 附近）：

现有：
```rust
                        if ui.button(def_label).clicked() {
                            actions.push(SidebarAction::OpenRoutine { node: node.clone(), force_new_tab: false });
                            ui.close();
                        }
```

改为（保持"在新标签页中打开"不变）：
```rust
                        if ui.button(def_label).clicked() {
                            actions.push(SidebarAction::OpenTableSummary {
                                connection_id: node.connection_id.clone(),
                                database: node.database.clone().unwrap_or_default(),
                                schema: node.schema.clone(),
                                db_label: node.database.clone().unwrap_or_default(),
                                ddl_target: Some(node.clone()),
                            });
                            ui.close();
                        }
```

- [ ] **Step 5: 处理 `OpenTableSummary` 的 `ddl_target`**

在 `SidebarAction::OpenTableSummary` 处理（app.rs:6158 附近）的表信息页创建/定位逻辑之后（无论新建还是复用 Tab，此时 `self.active_tab` 已指向目标 TableSummary Tab），追加目标设置与异步加载。该分支的尾部追加（在 match 分支的 `if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.get_mut(self.active_tab) { ... }` 结构之后，或直接并入同一结构内追加字段赋值与加载代码）：

```rust
                SidebarAction::OpenTableSummary { connection_id, database, schema, db_label, ddl_target } => {
                    // ...现有逻辑：找到或创建 TableSummary Tab（复用现有代码，勿改动）...
                    // ---- 追加开始：ddl_target 设置与异步加载 ----
                    if let Some(target) = ddl_target {
                        if let Some(WorkspaceTab::TableSummary(tab)) = self.tabs.get_mut(self.active_tab) {
                            tab.ddl_panel_visible = true;
                            tab.ddl_loading = true;
                            tab.ddl_text = None;
                            tab.ddl_error = None;
                            tab.ddl_target = Some(if matches!(target.node_type, ExplorerNodeType::Procedure | ExplorerNodeType::Function) {
                                DdlTarget::Routine(target.clone())
                            } else {
                                DdlTarget::Table(target.clone())
                            });
                            let tab_id = tab.id.clone();
                            let node = target.clone();
                            let services = self.services.clone();
                            let (sender, receiver) = mpsc::channel();
                            self.pending_ddl_load.insert(tab_id.clone(), receiver);
                            let is_routine = matches!(node.node_type, ExplorerNodeType::Procedure | ExplorerNodeType::Function);
                            let is_proc = matches!(node.node_type, ExplorerNodeType::Procedure);
                            let connection_id = node.connection_id.clone();
                            let database = node.database.clone();
                            let schema = node.schema.clone();
                            let name = node.name.clone();
                            self.runtime.handle().spawn(async move {
                                let result = if is_routine {
                                    let routine = core_domain::RoutineRef {
                                        connection_id,
                                        database,
                                        schema,
                                        name,
                                        is_procedure: is_proc,
                                    };
                                    services.load_routine_definition(&routine).await.map(|d| d.create_sql.unwrap_or_default())
                                } else {
                                    let table = TableRef {
                                        connection_id,
                                        database,
                                        schema,
                                        table: name,
                                        is_view: matches!(node.node_type, ExplorerNodeType::View),
                                    };
                                    services.load_table_definition(&table).await.map(|d| d.create_sql.unwrap_or_default())
                                };
                                let _ = sender.send(DdlLoadResult {
                                    tab_id,
                                    result: result.map_err(|e| e.to_string()),
                                });
                            });
                        }
                    }
                }
```

注意：该代码块与 Task 6 Step 2 的 `ShowDdl` 处理逻辑完全一致（除 `tab.ddl_panel_visible` 的初始设置不同外）。两处都保持**内联**，不要抽成 `&mut self` 方法——在 `self.tabs.get_mut(...)` 块内调用 `&mut self` 方法会整体借用 `self`，与 `tab` 的可变借用冲突。`self.pending_ddl_load.insert(...)`、`self.runtime.handle().spawn(...)` 是 `self` 的不相交字段访问，可正常编译（同 app.rs:7747 `LoadRoutines` 模式）。执行本 Task 时若代码与 Task 6 存在差异，以本 Task 块为准。

- [ ] **Step 6: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): 侧边栏右键查看定义联动 DDL 面板"
```

---

### Task 8: 选中行变化时面板联动

**Files:**
- Modify: `apps/desktop/src/app.rs`

**Interfaces:**
- Consumes:
  - 行渲染的点击/双击事件（app.rs:12413-12422 附近）
  - `summary_to_node`（已有）
  - `SummaryContextAction::ShowDdl`（Task 2）
- Produces: 单击/双击行时若面板可见，自动加载该行 DDL

- [ ] **Step 1: 在行点击/双击处理中触发 DDL 加载**

在 `render_table_summary_tab` 的行渲染点击处理（app.rs:12415 附近）：

现有：
```rust
                                                    if response.double_clicked() {
                                                        tab.pending_open_table = Some(ri);
                                                    } else if response.clicked() {
                                                        if modifiers.shift {
                                                            extend_summary_selection(tab, ri);
                                                        } else if modifiers.ctrl || modifiers.command {
                                                            toggle_summary_selection(tab, ri);
                                                        } else {
                                                            set_single_summary_selection(tab, ri);
                                                        }
                                                    }
```

在 `response.clicked()` 分支末尾追加（仅在面板可见且非多选/非 Ctrl 场景时联动，避免多选时反复加载）：

```rust
                                                    if response.double_clicked() {
                                                        tab.pending_open_table = Some(ri);
                                                    } else if response.clicked() {
                                                        if modifiers.shift {
                                                            extend_summary_selection(tab, ri);
                                                        } else if modifiers.ctrl || modifiers.command {
                                                            toggle_summary_selection(tab, ri);
                                                        } else {
                                                            set_single_summary_selection(tab, ri);
                                                            // 面板可见时，单机选中行 → 联动展示该行 DDL
                                                            if tab.ddl_panel_visible {
                                                                if let Some(s) = Self::summary_display_items(tab).get(ri) {
                                                                    let node = summary_to_node(s, tab);
                                                                    tab.pending_actions.push(SummaryContextAction::ShowDdl { node });
                                                                }
                                                            }
                                                        }
                                                    }
```

注意：这会与 Task 4 中"展开时自动加载选中行"逻辑重叠。为保持行为一致，Task 4 Step 2 的展开逻辑保留（展开瞬间无目标时加载选中行）；本 Task 处理"面板已展开后用户切换选中行"的联动。二者通过 `tab.ddl_target` 变化触发相同的 `ShowDdl` 动作，不会冲突（每次 `ShowDdl` 都会重置 `ddl_*` 状态并重新加载）。

- [ ] **Step 2: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat(summary): 选中行切换联动 DDL 面板"
```

---

### Task 9: i18n 翻译补充

**Files:**
- Modify: `crates/i18n/src/lib.rs`

**Interfaces:**
- Consumes: Task 4/5/6 中新增的 `tr!` 字符串 key
- Produces: `en()` 函数中的英文翻译

- [ ] **Step 1: 在 `en()` 中添加翻译**

在 `crates/i18n/src/lib.rs` 的 `en()` 函数中新增（key 必须与代码中的中文字符串完全一致）：

```rust
        "查看 DDL 面板" => "View DDL Panel",
        "请先在表格中选择一个对象" => "Select an object in the table first",
```

先搜索确认这些 key 是否已存在：

Run: `grep -n "查看 DDL 面板\|请先在表格中选择一个对象" crates/i18n/src/lib.rs`
Expected: 无输出（不存在，需新增）。若已存在则跳过对应条目。

同时确认 `render_summary_ddl_panel` 中引用的 `tr!("正在加载定义...")`、`tr!("加载失败")`、`tr!("加载失败：任务异常终止")` 均已存在（前三个 grep 已确认存在于 app.rs / i18n，无需新增）。

- [ ] **Step 2: 验证编译**

Run: `cargo check -p desktop`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/i18n/src/lib.rs
git commit -m "i18n: DDL 面板文案翻译"
```

---

### Task 10: 端到端验证与手动测试

**Files:**
- 无代码修改（仅验证）

**Interfaces:**
- Consumes: 全部 Task 1-9 的产出

- [ ] **Step 1: 完整构建**

Run: `cargo build -p desktop`
Expected: BUILD SUCCESSFUL，无 warning 中与本次改动相关的新增项

- [ ] **Step 2: 手动测试清单**

按以下清单逐项验证（需要运行 `cargo run -p desktop` 连接一个数据库）：

1. **工具栏图标**：表信息页工具栏出现新图标，点击展开右侧 DDL 面板，再点收起。无选中行时展开 → 显示"请先在表格中选择一个对象"
2. **右键查看定义**：选中一行表 → 右键"查看表定义" → 面板展开并展示该表的格式化 DDL
3. **存储过程/函数**：切到"存储过程"标签 → 右键"查看存储过程定义" → 面板展示存储过程 DDL
4. **选中行联动**：面板已展开时单击另一行 → 面板刷新为最新行的 DDL
5. **拖拽调宽**：面板左边缘拖拽调整宽度（150–500px 范围），收起到其他 Tab 再回来，宽度保持
6. **侧边栏右键**：侧边栏对象树右键表 → "查看表定义" → 自动打开/定位表信息页并展开面板展示 DDL；右键存储过程同理
7. **回归**：表信息页双击行打开数据页、右键"在新标签页中打开"、筛选/搜索/排序、状态栏计数均正常

- [ ] **Step 3: 记录验证结果**

将手动测试结果（每项通过/失败）记录在本会话中。若发现 bug，按 `superpowers:systematic-debugging` 流程修复后再验证。

- [ ] **Step 4: 完成**

全部清单通过后，向用户报告功能已完成。
