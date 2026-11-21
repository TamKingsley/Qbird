# Qbird

[![版本](https://img.shields.io/badge/version-0.1.14-orange)](https://github.com/TamKingsley/Qbird/releases) [![平台](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-blue)] [![许可证](https://img.shields.io/github/license/TamKingsley/Qbird)](LICENSE)

**面向局域网设备直连的本地优先聊天工具。**

[English](README.md) · 简体中文

Qbird 是一款 Windows 桌面应用，可发现附近的 Qbird 用户并让设备之间直接通信。应用不依赖托管中转服务器或账号服务；聊天记录与身份数据保存在本机。

> **0.1.14 · Windows x64 预览版。** Qbird 尚未经过独立安全审计。本地 SQLite 数据库未加密，应用锁仅隐藏界面。请勿将此预览版用于敏感信息或生产通信。局域网发现会广播应用中设置的昵称和用户名。

## 界面预览

![Qbird 0.1.14 消息页面](docs/assets/qbird-demo.png)

*截图来自当前版本的隔离本机配置，不含个人数据。*

## 当前功能

- 局域网发现、联系人申请、身份指纹与单聊；也可在需要时通过已知地址添加对方。
- 单聊、群聊和自聊，支持本地记录、消息搜索、回复与群组 @ 提及。
- 本地文件和图片传输、失败重试与续传、可配置接收目录及附件存储配额。
- 新消息提醒、系统托盘控制，以及最小化到托盘后在后台继续运行。
- 加密且受密码保护的聊天备份，以及当前会话聊天记录的导入/导出。备份不包含身份私钥和附件文件。
- 标签式工作区，内置图片、PDF、文本/日志、CSV、JSON、Markdown、禁用脚本的 HTML 和网页链接预览。额外的本地预览插件仍属实验性只读功能，见[插件说明](docs/PLUGIN.md)。
- 界面框架及部分区域可选简体中文、繁体中文和英文；聊天页面与部分后端提示尚未完全翻译。

Qbird 使用 Tauri、Rust、TypeScript、Vite 和 SQLite 构建，点对点通信不经过托管中转服务器。预览插件的权限边界见[插件说明](docs/PLUGIN.md)。

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
