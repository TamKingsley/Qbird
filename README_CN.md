# Qbird · 青鸟

**轻巧、安全导向、本地优先的局域网聊天工具，开箱即用，并通过插件扩展。**

[English](README.md) · 简体中文

Qbird 中文名为 **青鸟**。项目愿景是让局域网中的直接通信轻巧、易部署，不依赖托管中转服务器或账号服务。核心保持小而专注，只承担身份、发现、消息和本地数据等基础能力；基础之外的能力以插件扩展，让用户按需安装，而不是让所有功能都进入核心。当前优先开发 Windows 版本；Linux 和 macOS 仍在规划中。

> **0.1.12-beta 开发测试版**：当前为预发布的 beta 版本，不是正式产品。真实双机、多机及当前桌面界面验收尚未完成；本地数据库未加密，也尚未经过独立安全审计。**请勿用于真实通信或存储敏感信息。**「默认保护隐私」是设计目标，并不代表此 beta 已通过安全审计。

## 界面预览

![Qbird 局域网发现页面，显示内置的虚构演示联系人](docs/assets/qbird-demo.png)

*当前 0.1.12-beta 的发现界面，使用内置演示数据。画面中的联系人、姓名和私网地址均为虚构，不代表实时网络，也不含真实聊天记录。*

## 功能介绍

以下能力已在开发版中不同程度地实现。自动化测试与真实环境验收不同，具体状态见[任务清单](docs/TASKS.md)。

| 功能 | 开发版现状 |
| --- | --- |
| **首次使用与发现** | 三步首次引导；同网段 mDNS 发现、手动地址、限速网段扫描和黑名单。添加联系人前探测对方指纹；可关闭自动接受联系人。 |
| **聊天会话** | 本地自聊、单聊和群聊，支持隐藏会话、未读计数、搜索、消息引用和群内 @ 成员；文字与最多 9 张图片可组成同一气泡。 |
| **联系人与群组** | 联系人资料、备注与分组；群邀请、成员管理与进群时间、按群名搜索与申请加入、群头像。群邀请默认需确认，可选自动加入。 |
| **文件与图片** | 单聊和群聊文件排队传输、断点续传与 SHA-256 校验；可配置接收目录及确认策略；支持图片查看和工作区预览，已完成的附件可用系统默认程序打开。 |
| **工作区与扩展** | 全局标签式右侧工作区支持图片（可缩放旋转）、PDF（依赖 WebView2 内置阅读器）、TXT/Markdown/HTML 与网页链接预览，标签可拖拽排序、左右分屏，并在重启后恢复布局；常用表情、本地表情收藏、输入设置、应用锁和存储配额。已提供实验性的本地**只读预览插件宿主**，但未内置 Office 文档查看器插件。 |
| **导出与备份** | 聊天记录纯文本导出、密码保护的导入导出；密码保护的数据库备份以及合并或覆盖恢复。数据库备份不包含身份私钥与附件。 |
| **调试与自检** | 设置内「调试模式」：只读的本机网络检测（监听、端口、局域网发现、IPv4），有界修复（重启聊天服务、重建局域网发现），以及空间检测（附件配额、数据库大小、接收目录磁盘余量）。 |

Qbird 使用 **Tauri、Rust、TypeScript、Vite 和 SQLite** 构建。记录保存在本机；点对点通信不使用中转服务器。

## 项目愿景：小核心，插件优先

长期方向是让 Qbird 的基础架构轻巧、可靠，再将可选能力作为插件添加，而不是每增加一种场景就扩张核心。路线图示例包括更丰富的侧边栏文件预览插件、AI Bot 接入插件；这些**目前只是设想，并非已交付功能**。目前应用带有实验性的本地只读预览插件宿主，尚不是稳定通用的插件 API，也没有插件市场。当前边界见[插件说明](docs/PLUGIN.md)。

## 从源码部署（Windows x64）

当前只有 Windows x64 支持源码部署与运行。以下步骤在 Windows 10/11、已安装 Visual Studio 生成工具与 Rust 的环境中使用。

### 1. 准备工具链

| 组件 | 要求 | 用途 / 说明 |
| --- | --- | --- |
| Windows | 10 或 11 x64 | 唯一受支持的平台 |
| Microsoft C++ 生成工具 | Visual Studio 2022 Build Tools，勾选「使用 C++ 的桌面开发」及 Windows 10/11 SDK | 编译 Rust 并链接 MSVC 运行库 |
| Rust | `stable-x86_64-pc-windows-msvc`（由 [rustup](https://rustup.rs/) 安装），且 `cargo` 在 `PATH` 中 | 后端与桌面外壳 |
| Node.js | 22 LTS 或更高（CI 使用 Node 22） | 前端构建 |
| pnpm | 11.22.0（见 `package.json` 的 `packageManager` 字段） | 可用 `corepack enable` 或 `npm i -g pnpm@11.22.0` 安装 |
| WebView2 Runtime | 常青版 | Windows 11 通常自带；缺失时安装包会引导下载安装程序 |

安装完成后在终端确认工具链可用：

```bash
node -v && pnpm -v && cargo -V && rustc -V
```

### 2. 获取代码并安装依赖

```bash
git clone https://github.com/TamKingsley/Qbird.git
cd Qbird
pnpm install --frozen-lockfile
```

依赖版本由 `pnpm-lock.yaml` 与 `src-tauri/Cargo.lock` 锁定；如提示 `node_modules` 与锁文件不一致，重新执行上面的 `pnpm install --frozen-lockfile`，不要手改锁文件。

### 3. 开发运行

```bash
pnpm tauri dev
```

首次运行需要完整编译 Rust 依赖，耗时较长；之后为增量编译。Vite 开发服务器固定监听 `127.0.0.1:1420`（`strictPort`），被占用时直接报错而不是换端口。

只预览界面（不启动桌面外壳，无法收发消息或读写本地数据库）：

```bash
pnpm dev            # 然后浏览器打开 http://127.0.0.1:1420/?demo=1
```

`?demo=1` 会用内置演示数据渲染「局域网用户」页，仅用于看界面；桌面运行时忽略该参数。

### 4. 检查与测试

```bash
pnpm typecheck     # tsc 类型检查
pnpm build         # 版本一致性校验 + 类型检查 + 前端生产构建
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

`pnpm build` 会先执行 `scripts/check-version.mjs`：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 与 `src-tauri/Cargo.lock` 的版本必须完全一致（允许 `-beta` 之类预发布后缀），否则构建失败——发版前先统一这四处版本号。

Rust 测试中依赖 Windows DPAPI、真实回环网络与桌面会话的用例，在无交互桌面（部分 CI、远程或沙箱会话）中会失败并报「系统找不到指定的文件」或连接错误；请在已登录的正常桌面会话里跑完整 `cargo test`，以本地结果为准。

### 5. 构建可分发版本

先生成前端产物，再编译 release 可执行文件：

```bash
pnpm build
cargo build --release --features custom-protocol
```

产物为 `src-tauri/target/release/qbird.exe`：已内嵌前端资源，可直接双击运行。

生成 NSIS 安装包（当前用户安装，含卸载钩子）：

```bash
pnpm tauri build            # 或： pnpm tauri bundle --bundles nsis
```

安装包位于 `src-tauri/target/release/bundle/nsis/`。构建 NSIS 需要 NSIS 工具链，缺失时 `tauri bundle` 会报错，此时可只交付上面的 `qbird.exe`。若 `pnpm tauri build` 在解析 Rust 工程阶段异常退出，可跳过它：先执行 `pnpm build` 与 `cargo build --release --features custom-protocol` 得到 exe，再单独运行 `pnpm tauri bundle --bundles nsis`。

构建产物、安装包、`dist/` 与 `src-tauri/target/` 都不入库。

### 6. 数据目录与便携模式

- 用安装包安装后，安装器会在程序目录写入 `qbird.portable` 标记，数据（身份、数据库、附件、插件、表情）统一保存在安装目录下的 `data\`；卸载时按提示备份到 `文档\Qbird 数据` 或彻底删除。
- 直接运行 `qbird.exe` 或用 `pnpm tauri dev` 时，若程序目录没有 `qbird.portable`，数据保存在系统应用数据目录（标识 `im.qbird.desktop`）。
- 备份、迁移或排查问题前先退出应用；`data\`、身份私钥、`*.db` 与备份文件都不要提交进仓库（`.gitignore` 已覆盖常见后缀）。

### 7. 常见问题

- **`cl.exe` 报 `C1083: 无法打开包括文件: “stddef.h”` 或找不到 `windows.h`**：`vcvars64.bat` 没有把 Windows SDK 的 include/lib 注入环境。手动补上 `%WindowsSdkDir%\Include\<版本>\ucrt;...\um;...\shared` 与 `%WindowsSdkDir%\Lib\<版本>\ucrt\x64;...\um\x64`，或重装「Windows 10/11 SDK」组件后重开终端。
- **`cargo` 报 `STATUS_DLL_NOT_FOUND` 或 `rustc` 异常**：多为 rustup 代理的 `rustc` 被拦截。确认使用工具链内的真实 `rustc`（`%USERPROFILE%\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin`），必要时执行 `rustup toolchain install stable-x86_64-pc-windows-msvc` 后重试。
- **找不到 `link.exe`**：当前终端没有 MSVC 环境。请在「x64 Native Tools Command Prompt for VS 2022」中执行，或先调用 `vcvars64.bat`。
- **端口 1420 被占用**：`pnpm dev` 使用 `strictPort`，请先释放端口；不要改端口，否则会与 `tauri.conf.json` 的 `devUrl` 不一致。
- **界面白屏或窗口无法创建**：确认 WebView2 Runtime 已安装且当前会话有可用的交互桌面；无桌面会话下 WebView2 可能报 `0x800700AA`。
- **版本校验失败**：按第 4 节同步四处版本号后重跑 `pnpm build`。

数据格式、分发方式与后续计划的更多细节见[产品需求文档](docs/PRD.md)。

## 当前限制

- **验收：**真实双机、群聊并发、异常网络及当前桌面界面验收尚未完成；本机或自动化测试通过不代表达到生产可用标准。
- **离线补发：**没有中转服务器，待发送消息或文件需相关双方同时在线才能补发或续传。
- **本地数据：**SQLite 数据库尚未加密；应用锁只隐藏界面，不加密存储数据，Windows 身份验证重置尚未实现。
- **备份范围：**数据库备份不包含身份私钥和附件；备份密码与应用锁密码独立。
- **扩展与平台：**插件宿主为实验性只读预览，不提供编辑、插件市场或内置文档查看器；Linux 和 macOS 尚未支持。

## 安全与隐私

Windows 上的本机身份私钥由 DPAPI 保护，单聊使用双向认证 TLS。局域网发现会广播用户设置的昵称与用户名；发现信息和网络地址本身不代表可信身份。当前开发版默认自动接受联系人通知，可在设置中关闭。依赖联系人身份前，应通过独立可靠的渠道核对完整指纹。传输加密不等于本地数据加密。应用未配置分析或遥测服务；但点击网页链接会在内嵌 WebView 中直接加载该网站，网站可能看到你的网络地址和请求元数据。请勿提交身份密钥、聊天数据库、备份或机器私有配置。

## 项目进度

已实现功能、测试状态与待验收事项见[任务清单](docs/TASKS.md)，预期范围见[产品需求文档](docs/PRD.md)，实验性预览接口见[插件文档](docs/PLUGIN.md)。规划中的功能不代表已经交付。

## 参与贡献

Qbird 目前是 **beta 版，仍在持续开发中**，界面、协议和数据格式都可能调整，暂不建议用于生产环境。现正开放社区审阅与共创，尤其欢迎对核心/插件边界的意见。

- 如果你认同这个方向，欢迎给仓库点 **Star**；也可以 Fork 试验，并通过 [Issue](https://github.com/TamKingsley/Qbird/issues) 报告问题或提出功能需求。
- 欢迎一起共创 PR。请先阅读[产品需求文档](docs/PRD.md)与[任务清单](docs/TASKS.md)，保持单个 PR 聚焦；提交前运行 `pnpm build` 与 `cargo check --manifest-path src-tauri/Cargo.toml`。涉及身份、加密、通信协议或数据迁移的改动，建议先在 Issue 中说明方案。

## 许可证

本项目采用 [Apache License 2.0](LICENSE)。第三方依赖遵循各自许可证，可查看依赖包的元数据与上游声明。本仓库不包含发布安装包或编译后的二进制文件。
