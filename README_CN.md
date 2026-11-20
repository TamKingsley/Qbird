# QBird（暂定名称：青鸟）

**轻巧、本地优先的局域网聊天工具。**

[English](README.md) · [简体中文](README_CN.md)

QBird 是一款开源桌面聊天工具，面向局域网内的直接通信。目标是无需托管中转服务器或账号服务即可开箱运行，并让数据保存在用户自己的设备上。

> **0.1.12-beta · Windows x64 开发预览版。** 项目尚未经过独立安全审计，本地数据库未加密。请勿将此 beta 版用于敏感信息或生产通信。

## 界面预览

![Qbird 局域网发现页面，使用虚构演示数据](docs/assets/qbird-demo.png)

*画面中的联系人和网络地址均为虚构演示数据。*

## 项目方向

基础架构计划保持轻巧，聚焦身份、局域网发现、聊天与本地数据。可选能力应通过插件增加，而不是不断扩张核心。更丰富的侧边栏文件预览和 AI Bot 接入是路线图示例，**目前尚未交付**。当前应用包含实验性的本地只读预览插件宿主，但还不是稳定通用的插件 API。

## 当前功能

- 局域网发现、联系人与身份识别。
- 自聊、群聊、本地聊天记录及文件/图片传输。
- 本地导出与备份工具，以及支持格式的标签式预览工作区。
- 实验性的只读预览插件宿主，见[插件说明](docs/PLUGIN.md)。

Qbird 使用 Tauri、Rust、TypeScript、Vite 和 SQLite 构建。点对点通信不依赖托管中转服务器。Windows 版的局域网发现会广播用户设置的昵称和用户名。

## 从源码运行

**当前支持平台：** Windows 10/11 x64。需要 Visual Studio 2022 C++ Build Tools（含 Windows SDK）、Rust（`stable-x86_64-pc-windows-msvc`）、Node.js 22 及以上、pnpm 11.22.0 和 WebView2 Runtime。

```bash
git clone https://github.com/TamKingsley/Qbird.git
cd Qbird
pnpm install --frozen-lockfile
pnpm tauri dev
```

## 参与共创

Qbird 欢迎社区审阅与共创。如果你认同这个方向，欢迎给仓库点 **Star**、Fork 试验，或通过 [Issue](https://github.com/TamKingsley/Qbird/issues) 报告问题、提出需求。欢迎提交聚焦的 PR；涉及身份、密码学或通信协议的改动，请先在 Issue 讨论。

## 许可证

本项目采用 [Apache License 2.0](LICENSE)。第三方依赖遵循各自许可证。
