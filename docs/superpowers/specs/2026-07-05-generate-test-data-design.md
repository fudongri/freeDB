# 生成测试数据功能设计

**日期**：2026-07-05
**范围**：数据页面（Table Tab）增加"生成数据"功能

## 概述

在数据页面工具栏的"列"按钮右侧增加"生成数据"按钮，点击弹出配置面板，支持批量生成随机测试数据，显示进度条，可中途停止。

## 支持的数据库

MySQL、PostgreSQL、MongoDB。

## UI 设计

### 按钮位置

在"列"按钮（`render_table_tab` Data 分支）右侧，使用 `MiniButtonKind::Subtle` 样式，标签为 `tr!("生成数据")`。

### 配置面板（popup）

点击按钮后弹出，内容：
- 标题：`tr!("生成测试数据")`
- 数量输入框：`tr!("生成数量")` + 数字输入 + `tr!("行")`
- 跳过列提示：自动识别 auto_increment 列并显示
- 开始按钮：`tr!("开始生成")`

### 进度面板

点击"开始生成"后，面板切换为：
- 标题：`tr!("正在生成测试数据...")`
- 进度条：`completed / total`
- 停止按钮：`tr!("停止")`

### 完成状态

显示 `tr!("已生成 {} 条测试数据", count)`，点击空白处关闭，自动刷新表数据。

## 数据生成逻辑

### 类型映射

| 类型模式 | 生成规则 |
|---------|---------|
| `int/bigint/smallint/tinyint` | 随机整数，考虑字段大小范围 |
| `varchar/text/char` | `"test_"` 前缀 + 随机字符串 |
| `float/double/decimal/numeric` | 随机浮点数 |
| `date` | 近一年内随机日期 |
| `datetime/timestamp` | 近一年内随机时间 |
| `boolean/bit` | 随机 true/false |
| `json/jsonb` | `{"test": true}` |
| `uuid` | 随机 UUID |
| 其他 | `"test_value"` |

### 特殊处理

- **auto_increment 列**：跳过，不出现在 INSERT 中
- **nullable 列**：约 10% 概率生成 NULL
- **有 default_value 的列**：30% 概率使用 DEFAULT（不插入该列）
- **所有字符串**：加 `"test_"` 前缀，标识测试数据
- **唯一约束**：忽略，依赖数据库处理冲突

### MongoDB 处理

- 先 `find().limit(10)` 采样现有文档
- 推断每个字段的类型（String/Number/Boolean/Array/Object）
- 按推断类型生成随机数据

## 异步执行机制

### 新增状态（TableTabState）

```rust
show_generate_data_popup: bool,
generate_data_count: String,
generate_data_running: bool,
generate_data_progress: Option<GenerateDataProgress>,
generate_data_receiver: Option<Receiver<GenerateDataEvent>>,
generate_data_cancel: Option<Arc<AtomicBool>>,
```

### 新增类型

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

### 执行流程

1. 用户点击"开始生成" → 解析数量，创建 channel 和 cancel 标志
2. `tokio::spawn` 启动任务，每批 100 行：
   - 检查 cancel 标志 → 已取消则发送 `Done` 并退出
   - 生成一批数据，执行多行 INSERT（SQL）或 `insert_many`（MongoDB）
   - 发送 `Progress`
3. UI 每帧 poll receiver，更新进度条
4. 点击"停止" → 设置 cancel 标志，当前批次完成后停止

### 批次大小

MySQL/PostgreSQL：100 行/批
MongoDB：100 条/批

## 文件改动

- `apps/desktop/src/app.rs`：
  - `TableTabState` 增加生成数据相关字段
  - `render_table_tab` Data 分支增加按钮和 popup 渲染
  - 新增 `GenerateDataProgress`、`GenerateDataEvent` 类型
  - 新增生成数据的异步任务启动和 poll 逻辑
- `crates/core-domain/src/lib.rs`：如有需要，增加类型辅助方法
