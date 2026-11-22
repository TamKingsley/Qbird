# Qbird

[![发布版本](https://img.shields.io/github/v/release/TamKingsley/Qbird?include_prereleases&label=release&color=orange)](https://github.com/TamKingsley/Qbird/releases) [![平台](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-blue)](https://github.com/TamKingsley/Qbird/releases) [![Rust](https://img.shields.io/badge/Rust-stable-informational?logo=rust&logoColor=white)](https://www.rust-lang.org) [![TypeScript](https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org) [![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=white)](https://tauri.app) [![许可证](https://img.shields.io/github/license/TamKingsley/Qbird)](LICENSE)

**面向局域网设备直连的本地优先聊天工具。**

[English](README.md) · 简体中文

Qbird 是一款 Windows 桌面应用，可发现附近的 Qbird 用户并让设备之间直接通信。应用不依赖托管中转服务器或账号服务；聊天记录与身份数据保存在本机。

> **0.1.15 · Windows x64 预览版。** Qbird 尚未经过独立安全审计。本地 SQLite 数据库未加密，应用锁仅隐藏界面。请勿将此预览版用于敏感信息或生产通信。局域网发现会广播应用中设置的昵称和用户名。

## 界面预览

![Qbird 0.1.15 消息页面](docs/assets/qbird-demo.png)

_截图来自当前版本的隔离本机配置，不含个人数据。_

## 当前功能

- 局域网发现、联系人申请、身份指纹与单聊；也可在需要时通过已知地址添加对方。
- 单聊、群聊和自聊，支持本地记录、消息搜索、回复与群组 @ 提及。
- 本地文件和图片传输、失败重试与续传、可配置接收目录及附件存储配额。
- 新消息提醒、系统托盘控制，以及最小化到托盘后在后台继续运行。
- 加密且受密码保护的聊天备份，以及当前会话聊天记录的导入/导出。备份不包含身份私钥和附件文件。
- 标签式工作区，内置图片、PDF、文本/日志、CSV、JSON、Markdown、禁用脚本的 HTML 和网页链接预览。额外的本地预览插件仍属实验性只读功能，见[插件说明](docs/PLUGIN.md)。
- 界面框架及部分区域可选简体中文、繁体中文和英文；聊天页面与部分后端提示尚未完全翻译。

Qbird 使用 Tauri、Rust、TypeScript、Vite 和 SQLite 构建，点对点通信不经过托管中转服务器。预览插件的权限边界见[插件说明](docs/PLUGIN.md)。

## 软件架构

Qbird 是 Tauri 应用：Rust 后端负责网络、存储与身份，TypeScript 前端负责界面渲染，两者通过 Tauri IPC 命令通信。

```text
+-------------------------------------------------------------+
|  前端（TypeScript / Vite）                                  |
|  main.ts · 单聊/群聊 · 截图 · 聊天记录 · 插件预览          |
|            |  invoke() / 事件（Tauri IPC）                  |
|  后端（Rust）                                               |
|  lib.rs · app_state · 监听 · 局域网发现 · 协议             |
|  storage.rs（SQLite）· backup.rs · identity.rs · files      |
+-------------------------------------------------------------+
```

### 安装后数据目录

所有用户数据均保存在本机，位于按用户隔离的数据目录 `%APPDATA%\im.qbird.desktop\`：

```text
%APPDATA%\im.qbird.desktop\        # 数据目录（identifier: im.qbird.desktop）
├── qbird.sqlite3                  # 聊天记录、联系人、会话（未加密）
├── identity.bin                   # 设备身份密钥，经 Windows DPAPI 保护
├── logs\
│   └── qbird.log                  # 轮转应用日志（含 panic 钩子）
├── attachments\
│   └── in\
│       ├── direct\                # 单聊接收文件
│       └── group\                 # 群聊接收文件
└── pre-restore-<时间戳>.db         # 覆盖式恢复前创建的临时快照
```

> SQLite 数据库与身份文件仅存在于本机，无托管中转或账号服务器。备份经密码加密，但不包含 `identity.bin`。

## 从源码运行

**当前支持的构建平台：** Windows 10/11 x64。需要 Visual Studio 2022 C++ Build Tools（含 Windows SDK）、Rust（`stable-x86_64-pc-windows-msvc`）、Node.js 22 及以上、pnpm 11.22.0 和 WebView2 Runtime。

```bash
git clone https://github.com/TamKingsley/Qbird.git
cd Qbird
pnpm install --frozen-lockfile
pnpm tauri dev
```

## 参与共创

欢迎提交聚焦的 PR 或通过 [Issue](https://github.com/TamKingsley/Qbird/issues) 报告问题。涉及身份、密码学或通信协议的改动，请先在 Issue 中讨论。

## 许可证

本项目采用 [Apache License 2.0](LICENSE)。第三方依赖遵循各自许可证。

## 辅助开发

辅助开发：MuseWork 与 Lingxi。
