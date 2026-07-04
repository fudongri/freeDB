# 生成测试数据 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在数据页面工具栏"列"按钮右侧增加"生成数据"按钮，支持批量生成随机测试数据，带进度条和停止功能。

**Architecture:** 所有改动集中在 `apps/desktop/src/app.rs`。新增 `GenerateDataEvent` enum 和 `GenerateDataProgress` struct 用于进度通信。生成逻辑直接在 spawn 的异步任务中完成，每批 100 行通过 `services.execute_sql()` 执行 INSERT。使用 `Arc<AtomicBool>` 实现取消。UI 通过 popup 面板展示配置和进度。

**Tech Stack:** Rust, egui, tokio, mpsc channel, Arc<AtomicBool>

## Global Constraints

- 所有用户可见文本使用 `tr!("中文")` 宏
- `services.clone()` 是 cheap 的（Arc 包裹），可安全 clone 到 async 任务
- `QueryExecution { connection_id, database, sql }` 用于 SQL 和 MongoDB（MongoDB 命令格式：`db.collection.insertMany([...])`）
- `TableRef` 提供 `connection_id`, `database`, `table` 字段
- `ColumnDefinition` 提供 `name`, `data_type`, `nullable`, `primary_key`, `auto_increment`, `default_value`
- `DatabaseKind` 有三个变体：`MySql`, `Postgres`, `MongoDb`
- 批次大小：100 行/批
- 异步任务通过 `mpsc::channel` 通信，在 `poll_background_tasks()` 中用 `try_recv()` 轮询

---

### Task 1: 新增数据类型和 TableTabState 字段

**Files:**
- Modify: `apps/desktop/src/app.rs:346-394` (TableTabState struct)
- Modify: `apps/desktop/src/app.rs` (新增类型定义，放在 TableTabState 附近)

**Interfaces:**
- Produces: `GenerateDataEvent`, `GenerateDataProgress` 类型
- Produces: `TableTabState` 新字段供后续 Task 使用

- [ ] **Step 1: 在 app.rs 中新增 GenerateDataEvent 和 GenerateDataProgress 类型**

在 `TableTabState` 定义之前（约 line 345 前）添加：

```rust
struct GenerateDataProgress {
    completed: usize,
    total: usize,
}

enum GenerateDataEvent {
    Progress { completed: usize, total: usize },
    Done(Result<usize, String>),
}
```

- [ ] **Step 2: 在 TableTabState 中增加生成数据相关字段**

在 `TableTabState` struct 末尾（`mongo_page_cursors` 字段之后，闭合 `}` 之前）添加：

```rust
    // 生成测试数据
    show_generate_data_popup: bool,
    generate_data_count: String,
    generate_data_running: bool,
    generate_data_progress: Option<GenerateDataProgress>,
    generate_data_receiver: Option<Receiver<GenerateDataEvent>>,
    generate_data_cancel: Option<Arc<AtomicBool>>,
```

- [ ] **Step 3: 在 open_table_tab 中初始化新字段**

在 `apps/desktop/src/app.rs` 的 `open_table_tab` 方法中，`TableTabState` 初始化处（约 line 2066 `mongo_page_cursors: Vec::new()` 之后）添加：

```rust
            show_generate_data_popup: false,
            generate_data_count: "100".to_string(),
            generate_data_running: false,
            generate_data_progress: None,
            generate_data_receiver: None,
            generate_data_cancel: None,
```

- [ ] **Step 4: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat: 添加生成测试数据相关的数据结构和状态字段"
```

---

### Task 2: 实现 SQL 数据生成逻辑

**Files:**
- Modify: `apps/desktop/src/app.rs` (新增函数)

**Interfaces:**
- Consumes: `ColumnDefinition` (from `TableDefinition.columns`), `DatabaseKind`, `TableRef`
- Produces: `fn generate_test_values_for_columns(...)` → `Vec<(Vec<String>, Vec<String>)>` (列名列表, 值列表) per row
- Produces: `fn build_generate_data_sql(...)` → `String` (multi-row INSERT)

- [ ] **Step 1: 实现单行随机值生成函数**

在 app.rs 中新增函数（建议放在 `open_table_tab` 方法附近）：

```rust
/// 根据列类型生成单行测试数据值
/// 返回 (参与插入的列名列表, 对应的值列表)
fn generate_test_row(
    columns: &[ColumnDefinition],
    database_kind: DatabaseKind,
    row_index: usize,
) -> (Vec<String>, Vec<String>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut col_names = Vec::new();
    let mut values = Vec::new();

    for col in columns {
        // 跳过自增列
        if col.auto_increment {
            continue;
        }
        // 有默认值的列，30% 概率跳过
        if col.default_value.is_some() && (row_index * 37 + col.name.len() * 13) % 100 < 30 {
            continue;
        }
        // nullable 列，10% 概率设为 NULL
        if col.nullable && (row_index * 41 + col.name.len() * 7) % 100 < 10 {
            col_names.push(col.name.clone());
            values.push("NULL".to_string());
            continue;
        }

        col_names.push(col.name.clone());
        let dt = col.data_type.to_lowercase();
        let value = generate_value_by_type(&dt, row_index, &col.name, database_kind);
        values.push(value);
    }

    (col_names, values)
}

fn generate_value_by_type(data_type: &str, row_index: usize, col_name: &str, db_kind: DatabaseKind) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    row_index.hash(&mut hasher);
    col_name.hash(&mut hasher);
    let seed = hasher.finish();

    // 整数类型
    if data_type.contains("int") || data_type.contains("serial") {
        let val = (seed % 100_000) as i64;
        return val.to_string();
    }
    // 浮点类型
    if data_type.contains("float") || data_type.contains("double")
        || data_type.contains("decimal") || data_type.contains("numeric")
        || data_type.contains("real") {
        let val = (seed % 100_000) as f64 / 100.0;
        return format!("{:.2}", val);
    }
    // 日期类型
    if data_type == "date" {
        let days = (seed % 365) as u32;
        return format!("'2025-{:02}-{:02}'", (days % 12) + 1, (days % 28) + 1);
    }
    // 日期时间类型
    if data_type.contains("datetime") || data_type.contains("timestamp") {
        let days = (seed % 365) as u32;
        let secs = (seed % 86400) as u32;
        return format!("'2025-{:02}-{:02} {:02}:{:02}:{:02}'",
            (days % 12) + 1, (days % 28) + 1,
            secs / 3600, (secs % 3600) / 60, secs % 60);
    }
    // 布尔类型
    if data_type.contains("bool") || data_type == "bit" {
        return if seed % 2 == 0 { "TRUE".to_string() } else { "FALSE".to_string() };
    }
    // JSON 类型
    if data_type.contains("json") {
        return "'{\"test\": true}'".to_string();
    }
    // UUID 类型
    if data_type.contains("uuid") {
        return format!("'test-{:08x}-{:04x}-{:04x}-{:04x}-{:012x}'",
            (seed >> 32) as u32, (seed >> 16) as u16, seed as u16,
            ((seed * 7) >> 16) as u16, seed.wrapping_mul(13));
    }
    // MongoDB ObjectId
    if data_type.contains("objectid") {
        return format!("{:024x}", seed);
    }
    // 默认：字符串，带 test_ 前缀
    format!("'test_{}_{}'", col_name, seed % 100000)
}
```

- [ ] **Step 2: 实现批量 INSERT SQL 构建函数**

```rust
/// 构建多行 INSERT 语句（MySQL/PostgreSQL）
fn build_batch_insert_sql(
    table: &TableRef,
    columns: &[ColumnDefinition],
    database_kind: DatabaseKind,
    start_index: usize,
    count: usize,
) -> String {
    let mut all_rows = Vec::new();
    let mut common_cols: Option<Vec<String>> = None;

    for i in 0..count {
        let (col_names, values) = generate_test_row(columns, database_kind, start_index + i);
        if common_cols.is_none() {
            common_cols = Some(col_names.clone());
        }
        all_rows.push(values);
    }

    let cols = common_cols.unwrap_or_default();
    let col_list = cols.join(", ");
    let table_name = format_table_name(table, database_kind);

    let values_str: Vec<String> = all_rows
        .iter()
        .map(|row| format!("({})", row.join(", ")))
        .collect();

    format!("INSERT INTO {} ({}) VALUES {}", table_name, col_list, values_str.join(", "))
}

fn format_table_name(table: &TableRef, db_kind: DatabaseKind) -> String {
    match db_kind {
        DatabaseKind::MySql => {
            if let Some(ref db) = table.database {
                format!("`{}`.`{}`", db, table.table)
            } else {
                format!("`{}`", table.table)
            }
        }
        DatabaseKind::Postgres => {
            if let Some(ref schema) = table.schema {
                format!("\"{}\".\"{}\"", schema, table.table)
            } else {
                format!("\"{}\"", table.table)
            }
        }
        DatabaseKind::MongoDb => table.table.clone(),
    }
}
```

- [ ] **Step 3: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat: 实现 SQL 测试数据生成逻辑"
```

---

### Task 3: 实现 MongoDB 数据生成逻辑

**Files:**
- Modify: `apps/desktop/src/app.rs` (新增函数)

**Interfaces:**
- Consumes: `TableRef`, `AppServices` (for sampling existing docs)
- Produces: `fn build_mongo_insert_command(...)` → `String` (db.collection.insertMany 命令)
- Produces: `async fn sample_mongo_fields(...)` → `Vec<(String, String)>` (字段名, 类型字符串)

- [ ] **Step 1: 实现 MongoDB 字段采样函数**

```rust
/// 从 MongoDB 集合中采样现有文档，推断字段类型
/// 返回 (字段名, 推断类型) 列表
async fn sample_mongo_fields(
    services: &AppServices,
    table: &TableRef,
) -> Vec<(String, String)> {
    let cmd = format!("db.{}.find().limit(5)", table.table);
    let execution = QueryExecution {
        connection_id: table.connection_id.clone(),
        database: table.database.clone(),
        sql: cmd,
    };
    match services.execute_sql(execution).await {
        Ok(result) => {
            // 从 result.rows 中推断字段类型
            let mut field_types: Vec<(String, String)> = Vec::new();
            for (col_name, col_type) in result.columns.iter().zip(result.mongo_types.values()) {
                let type_str = match col_type.as_str() {
                    "string" => "string",
                    "double" | "int32" | "int64" | "decimal128" => "number",
                    "bool" => "boolean",
                    "array" => "array",
                    "object" => "object",
                    "objectId" => "objectid",
                    _ => "string",
                };
                field_types.push((col_name.clone(), type_str.to_string()));
            }
            // 如果没有采样到数据，返回 _id 字段
            if field_types.is_empty() {
                field_types.push(("_id".to_string(), "objectid".to_string()));
            }
            field_types
        }
        Err(_) => vec![("_id".to_string(), "objectid".to_string())],
    }
}
```

- [ ] **Step 2: 实现 MongoDB 批量插入命令构建**

```rust
/// 构建 MongoDB insertMany 命令字符串
fn build_mongo_insert_command(
    table: &TableRef,
    fields: &[(String, String)],
    start_index: usize,
    count: usize,
) -> String {
    let mut docs = Vec::new();
    for i in 0..count {
        let mut doc_fields = Vec::new();
        for (field_name, field_type) in fields {
            // 跳过 _id，让 MongoDB 自动生成
            if field_name == "_id" {
                continue;
            }
            let value = generate_mongo_value(field_type, start_index + i, field_name);
            doc_fields.push(format!("\"{}\": {}", field_name, value));
        }
        docs.push(format!("{{{}}}", doc_fields.join(", ")));
    }
    format!("db.{}.insertMany([{}])", table.table, docs.join(", "))
}

fn generate_mongo_value(field_type: &str, row_index: usize, field_name: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    row_index.hash(&mut hasher);
    field_name.hash(&mut hasher);
    let seed = hasher.finish();

    match field_type {
        "number" => (seed % 100_000).to_string(),
        "boolean" => if seed % 2 == 0 { "true".to_string() } else { "false".to_string() },
        "objectid" => format!("ObjectId(\"{:024x}\")", seed),
        "array" => "[\"test_item\"]".to_string(),
        "object" => "{\"test\": true}".to_string(),
        _ => format!("\"test_{}_{}\"", field_name, seed % 100_000),
    }
}
```

- [ ] **Step 3: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat: 实现 MongoDB 测试数据生成逻辑"
```

---

### Task 4: 实现异步任务启动和轮询

**Files:**
- Modify: `apps/desktop/src/app.rs` (DesktopApp impl，新增方法)

**Interfaces:**
- Consumes: `GenerateDataEvent`, `GenerateDataProgress` (from Task 1)
- Consumes: `build_batch_insert_sql`, `build_mongo_insert_command`, `sample_mongo_fields` (from Task 2, 3)
- Consumes: `TableTabState` fields (from Task 1)
- Produces: `fn start_generate_data(&mut self)` — 启动异步任务
- Produces: `fn poll_generate_data(&mut self)` — 轮询进度
- Produces: `fn stop_generate_data(&mut self)` — 停止任务

- [ ] **Step 1: 实现 start_generate_data 方法**

在 `DesktopApp` impl 中新增方法：

```rust
fn start_generate_data(&mut self) {
    let Some(WorkspaceTab::Table(tab)) = self.tabs.get_mut(self.active_tab) else {
        return;
    };
    let total: usize = match tab.generate_data_count.parse() {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let table = tab.table.clone();
    let database_kind = tab.database_kind;
    let definition = tab.definition.clone();
    let services = self.services.clone();
    let handle = self.runtime.handle().clone();

    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();

    tab.generate_data_running = true;
    tab.generate_data_progress = Some(GenerateDataProgress { completed: 0, total });
    tab.generate_data_receiver = Some(receiver);
    tab.generate_data_cancel = Some(cancel.clone());

    handle.spawn(async move {
        let batch_size = 100usize;

        if database_kind == DatabaseKind::MongoDb {
            // MongoDB 路径
            let fields = sample_mongo_fields(&services, &table).await;
            let mut completed = 0usize;
            while completed < total {
                if cancel.load(Ordering::Relaxed) {
                    let _ = sender.send(GenerateDataEvent::Done(Ok(completed))).await;
                    return;
                }
                let count = batch_size.min(total - completed);
                let cmd = build_mongo_insert_command(&table, &fields, completed, count);
                let execution = QueryExecution {
                    connection_id: table.connection_id.clone(),
                    database: table.database.clone(),
                    sql: cmd,
                };
                match services.execute_sql(execution).await {
                    Ok(_) => {
                        completed += count;
                        let _ = sender.send(GenerateDataEvent::Progress { completed, total }).await;
                    }
                    Err(e) => {
                        let _ = sender.send(GenerateDataEvent::Done(Err(e.to_string()))).await;
                        return;
                    }
                }
            }
            let _ = sender.send(GenerateDataEvent::Done(Ok(completed))).await;
        } else {
            // SQL 路径（MySQL / PostgreSQL）
            let Some(def) = definition else {
                let _ = sender.send(GenerateDataEvent::Done(Err("表定义未加载".to_string()))).await;
                return;
            };
            let mut completed = 0usize;
            while completed < total {
                if cancel.load(Ordering::Relaxed) {
                    let _ = sender.send(GenerateDataEvent::Done(Ok(completed))).await;
                    return;
                }
                let count = batch_size.min(total - completed);
                let sql = build_batch_insert_sql(&table, &def.columns, database_kind, completed, count);
                let execution = QueryExecution {
                    connection_id: table.connection_id.clone(),
                    database: table.database.clone(),
                    sql,
                };
                match services.execute_sql(execution).await {
                    Ok(_) => {
                        completed += count;
                        let _ = sender.send(GenerateDataEvent::Progress { completed, total }).await;
                    }
                    Err(e) => {
                        let _ = sender.send(GenerateDataEvent::Done(Err(e.to_string()))).await;
                        return;
                    }
                }
            }
            let _ = sender.send(GenerateDataEvent::Done(Ok(completed))).await;
        }
    });
}
```

- [ ] **Step 2: 实现 poll_generate_data 方法**

```rust
fn poll_generate_data(&mut self) {
    let Some(WorkspaceTab::Table(tab)) = self.tabs.get_mut(self.active_tab) else {
        return;
    };
    let Some(receiver) = tab.generate_data_receiver.take() else {
        return;
    };

    match receiver.try_recv() {
        Ok(GenerateDataEvent::Progress { completed, total }) => {
            tab.generate_data_progress = Some(GenerateDataProgress { completed, total });
            tab.generate_data_receiver = Some(receiver);
        }
        Ok(GenerateDataEvent::Done(result)) => {
            tab.generate_data_running = false;
            tab.generate_data_cancel = None;
            tab.generate_data_receiver = None;
            match result {
                Ok(count) => {
                    tab.generate_data_progress = Some(GenerateDataProgress {
                        completed: count,
                        total: count,
                    });
                    // 刷新表数据
                    self.pending_refresh_active_table = Some(false);
                }
                Err(e) => {
                    tab.error = Some(e);
                }
            }
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            tab.generate_data_receiver = Some(receiver);
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            tab.generate_data_running = false;
            tab.generate_data_cancel = None;
        }
    }
}
```

- [ ] **Step 3: 实现 stop_generate_data 方法**

```rust
fn stop_generate_data(&mut self) {
    let Some(WorkspaceTab::Table(tab)) = self.tabs.get_mut(self.active_tab) else {
        return;
    };
    if let Some(ref cancel) = tab.generate_data_cancel {
        cancel.store(true, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: 在 update() 中添加 poll_generate_data 调用**

在 `self.poll_background_tasks();`（line 9486）之后添加：

```rust
        self.poll_generate_data();
```

- [ ] **Step 5: 在 request_repaint 条件中添加 generate_data_running**

在 `if self.pending_connection_tree.is_some()` 条件链（line 9497）的 `||` 末尾添加：

```rust
            || self.tabs.iter().any(|t| matches!(t, WorkspaceTab::Table(t) if t.generate_data_running))
```

- [ ] **Step 6: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat: 实现生成测试数据的异步任务启动、轮询和停止"
```

---

### Task 5: 实现 UI — 按钮和配置/进度面板

**Files:**
- Modify: `apps/desktop/src/app.rs` (render_table_tab 函数，Data 分支)

**Interfaces:**
- Consumes: `start_generate_data`, `stop_generate_data` (from Task 4)
- Consumes: `TableTabState` fields (from Task 1)
- Produces: `TabUiAction::GenerateData` (new variant, handled in Task 6)

- [ ] **Step 1: 在 TabUiAction enum 中添加 GenerateData 变体**

在 `TabUiAction` enum（约 line 10343-10382）中添加：

```rust
    GenerateData,
```

- [ ] **Step 2: 在"列"按钮后面添加"生成数据"按钮**

在 `apps/desktop/src/app.rs` 的 `render_table_tab` 函数中，`column_btn_rect = Some(column_btn_response.rect);`（line 6781）之后添加：

```rust
                                            // 生成数据按钮
                                            let gen_data_btn_response = mini_button(ui, tr!("生成数据"), MiniButtonKind::Subtle);
                                            let mut gen_data_btn_rect = gen_data_btn_response.rect;
                                            if gen_data_btn_response.clicked() {
                                                tab.show_generate_data_popup = !tab.show_generate_data_popup;
                                            }
```

- [ ] **Step 3: 在列筛选 popup 之后添加生成数据 popup**

在列筛选 popup 渲染代码块（`if tab.show_column_filter { ... }`）之后，添加生成数据 popup：

```rust
                                            // 生成数据弹出面板
                                            if tab.show_generate_data_popup {
                                                let area_id = ui.make_persistent_id("generate_data_popup");
                                                egui::Area::new(area_id)
                                                    .order(egui::Order::Foreground)
                                                    .fixed_pos(egui::pos2(
                                                        gen_data_btn_rect.left(),
                                                        gen_data_btn_rect.bottom() + 4.0,
                                                    ))
                                                    .show(ui.ctx(), |ui| {
                                                        egui::Frame::window(ui.style())
                                                            .show(ui, |ui| {
                                                                ui.set_min_width(220.0);
                                                                if tab.generate_data_running {
                                                                    // 进度状态
                                                                    ui.label(RichText::new(tr!("正在生成测试数据...")).strong());
                                                                    ui.add_space(8.0);
                                                                    let progress = tab.generate_data_progress.as_ref();
                                                                    let fraction = progress
                                                                        .map(|p| if p.total > 0 { p.completed as f32 / p.total as f32 } else { 0.0 })
                                                                        .unwrap_or(0.0);
                                                                    ui.add(egui::ProgressBar::new(fraction).show_percentage());
                                                                    if let Some(p) = progress {
                                                                        ui.label(format!("{}/{}", p.completed, p.total));
                                                                    }
                                                                    ui.add_space(8.0);
                                                                    if ui.button(tr!("停止")).clicked() {
                                                                        action = TabUiAction::GenerateData; // 用同一个 action，handle_tab_action 中区分
                                                                    }
                                                                } else if let Some(ref progress) = tab.generate_data_progress {
                                                                    // 完成状态
                                                                    ui.label(RichText::new(
                                                                        tr!("已生成 {} 条测试数据", progress.completed)
                                                                    ).strong());
                                                                    ui.add_space(4.0);
                                                                    ui.label(RichText::new(tr!("数据为随机测试数据")).small().color(palette.text_secondary));
                                                                } else {
                                                                    // 配置状态
                                                                    ui.label(RichText::new(tr!("生成测试数据")).strong());
                                                                    ui.add_space(8.0);
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(tr!("生成数量"));
                                                                        ui.add(
                                                                            egui::TextEdit::singleline(&mut tab.generate_data_count)
                                                                                .desired_width(80.0)
                                                                        );
                                                                        ui.label(tr!("行"));
                                                                    });
                                                                    // 显示跳过的列
                                                                    if let Some(ref def) = tab.definition {
                                                                        let skipped: Vec<&str> = def.columns.iter()
                                                                            .filter(|c| c.auto_increment)
                                                                            .map(|c| c.name.as_str())
                                                                            .collect();
                                                                        if !skipped.is_empty() {
                                                                            ui.add_space(4.0);
                                                                            ui.label(
                                                                                RichText::new(tr!("跳过: {} (自增)", skipped.join(", ")))
                                                                                    .small()
                                                                                    .color(palette.text_secondary),
                                                                            );
                                                                        }
                                                                    }
                                                                    ui.add_space(8.0);
                                                                    if ui.button(tr!("开始生成")).clicked() {
                                                                        action = TabUiAction::GenerateData;
                                                                    }
                                                                }
                                                            });
                                                    });
                                            }
```

- [ ] **Step 4: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/app.rs
git commit -m "feat: 实现生成测试数据的 UI 按钮和配置/进度面板"
```

---

### Task 6: 连接 TabUiAction 和 i18n

**Files:**
- Modify: `apps/desktop/src/app.rs` (handle_tab_action)
- Modify: `crates/i18n/src/lib.rs` (en() 函数)

**Interfaces:**
- Consumes: `TabUiAction::GenerateData` (from Task 5)
- Consumes: `start_generate_data`, `stop_generate_data` (from Task 4)

- [ ] **Step 1: 在 handle_tab_action 中处理 GenerateData action**

在 `handle_tab_action` 函数中的 `match action` 块（约 line 3977+）添加新分支：

```rust
            TabUiAction::GenerateData => {
                let Some(WorkspaceTab::Table(tab)) = self.tabs.get(self.active_tab) else {
                    return;
                };
                if tab.generate_data_running {
                    self.stop_generate_data();
                } else {
                    self.start_generate_data();
                }
            }
```

- [ ] **Step 2: 在 i18n en() 函数中添加英文翻译**

在 `crates/i18n/src/lib.rs` 的 `en()` 函数的 `match` 块中添加：

```rust
        "生成数据" => "Generate",
        "生成测试数据" => "Generate Test Data",
        "生成数量" => "Count",
        "正在生成测试数据..." => "Generating test data...",
        "已生成 {} 条测试数据" => "Generated {} rows of test data",
        "数据为随机测试数据" => "Data is random test data",
        "开始生成" => "Start",
        "跳过: {} (自增)" => "Skip: {} (auto increment)",
```

- [ ] **Step 3: 确认编译通过**

Run: `cargo check -p freedb-desktop`
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/app.rs crates/i18n/src/lib.rs
git commit -m "feat: 连接生成数据 action 处理和 i18n 英文翻译"
```

---

### Task 7: 构建并验证

**Files:** 无新增修改

- [ ] **Step 1: 完整构建**

Run: `cargo build -p freedb-desktop`
Expected: 构建成功

- [ ] **Step 2: 启动应用验证功能**

Run: 启动 freedb desktop 应用
验证：
1. 打开一个表的数据页
2. 工具栏"列"按钮右侧出现"生成数据"按钮
3. 点击弹出配置面板，显示数量输入框和跳过的列
4. 输入数量点击"开始生成"，面板切换为进度状态
5. 进度条正确更新
6. 点击"停止"可中途停止
7. 完成后显示生成数量提示
8. 表数据自动刷新，新数据可见
9. 生成的数据字符串带 "test_" 前缀

- [ ] **Step 3: Commit（如有修复）**

```bash
git add -A
git commit -m "fix: 生成测试数据功能验证修复"
```
