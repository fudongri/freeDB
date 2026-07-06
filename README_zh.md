# FreeDB

[English](README.md) | [中文](README_zh.md)

---

跨平台 MySQL、PostgreSQL 和 MongoDB 桌面数据库客户端。

FreeDB 是一个使用 Rust 和 [egui](https://github.com/emilk/egui/) 构建的轻量、快速的数据库客户端，支持 macOS、Windows 和 Linux。

### 功能

- **多数据库** — 支持 MySQL、PostgreSQL 和 MongoDB
- **查询编辑器** — 带语法高亮、自动补全和多语句执行的 SQL 编辑器
- **多标签页** — 同时处理多个查询和连接
- **连接管理** — 保存、编辑、组织和分组数据库连接
- **连接池** — 缓存连接，支持健康检查和自动重试
- **SSL/TLS** — 可配置的 SSL 模式（禁用、首选、必须、验证 CA、验证完整）
- **表浏览器** — 在侧边栏浏览表、视图和模式
- **表视图** — 数据预览、表结构、索引和 DDL
- **数据筛选** — 支持 AND/OR 的多条件筛选，丰富的操作符
- **内联编辑** — 直接编辑单元格、插入行、删除行
- **建表** — 可视化创建表，支持定义列和索引
- **DDL 操作** — 创建/重命名/删除数据库、模式、表；清空表和复制表
- **保存查询** — 保存、重命名和管理常用 SQL
- **查询历史** — 持久化历史记录，带执行时间追踪
- **复制选项** — 复制为 INSERT 语句或 TSV
- **导出** — CSV、Excel (XLSX)、SQL 转储和 MongoDB Shell 脚本
- **深色模式** — 内置浅色和深色主题
- **缩放** — 可调节缩放比例（0.5x – 3.0x）
- **国际化** — 支持中英文，运行时切换
- **自动更新** — 应用内检查更新、下载并一键安装
- **跨平台** — macOS、Windows 和 Linux 构建

### 安装

**macOS (Homebrew)**

```bash
brew install --cask fudongri/tap/freedb
```

**Windows**

从 [Releases](https://github.com/fudongri/freeDB/releases) 下载安装包或便携版 ZIP。

### 环境要求

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)

### 构建

```bash
git clone https://github.com/fudongri/freeDB.git
cd freedb
cargo build --release
```

### 运行

```bash
cargo run --release
```

或直接运行二进制文件：

```bash
./target/release/freedb
```

### 数据存储位置

连接配置和历史记录仅保存在本地，不会上传。密码现阶段为明文存储——请将其视为本地开发工具使用。

| 平台 | 路径 |
|------|------|
| macOS | `~/Library/Application Support/freedb/` |
| Windows | `C:\Users\<用户名>\AppData\Local\freedb\` |
| Linux | `$XDG_DATA_HOME/freedb/` 或 `~/.local/share/freedb/` |

| 文件 | 内容 |
|------|------|
| `freedb.sqlite3` | 连接配置、查询历史、UI 状态 |
| `credentials.json` | 已保存的密码 |

### 项目结构

```
freedb/
├── apps/desktop/          # 桌面 GUI 应用 (egui/eframe)
├── crates/
│   ├── app-services/      # 应用服务层
│   ├── connection-pool/   # 连接池
│   ├── connection-store/  # 已保存连接的持久化
│   ├── core-domain/       # 共享领域类型
│   ├── driver-api/        # 数据库驱动抽象
│   ├── driver-mongodb/    # MongoDB 驱动
│   ├── driver-mysql/      # MySQL 驱动
│   ├── driver-postgres/   # PostgreSQL 驱动
│   ├── export-service/    # CSV、Excel、SQL 和 MongoDB 导出
│   ├── history-store/     # 查询历史
│   ├── i18n/              # 国际化（中文 / 英文）
│   ├── secure-store/      # 凭证存储
│   ├── session-manager/   # 会话生命周期
└── scripts/               # 构建和打包脚本
```

### Star History

[![Star History Chart](https://api.star-history.com/svg?repos=fudongri/freeDB&type=Date)](https://star-history.com/#fudongri/freeDB&Date)

### 许可证

MIT
