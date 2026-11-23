# Qbird

[![Release](https://img.shields.io/github/v/release/TamKingsley/Qbird?include_prereleases&label=release&color=orange)](https://github.com/TamKingsley/Qbird/releases) [![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-blue)](https://github.com/TamKingsley/Qbird/releases) [![Rust](https://img.shields.io/badge/Rust-stable-informational?logo=rust&logoColor=white)](https://www.rust-lang.org) [![TypeScript](https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org) [![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=white)](https://tauri.app) [![License](https://img.shields.io/github/license/TamKingsley/Qbird)](LICENSE)

**A local-first messenger for direct communication over a local network.**

English · [简体中文](README_CN.md)

Qbird is a Windows desktop app for discovering nearby Qbird users and communicating directly between devices. It has no hosted relay or account service; chat history and identity data are stored locally.

> **Windows x64 preview.** Qbird has not received an independent security audit. The local SQLite database is not encrypted, and App Lock only hides the interface. Do not use this preview for sensitive information or production communication. LAN discovery advertises the nickname and username configured in the app.

## Preview

![Qbird message screen](docs/assets/qbird-demo.png)

_The screenshots show the current build with an isolated local profile and no personal data._

## Highlights

A quick map of what Qbird does across its core areas.

| Area                    | What you get                                                                                             |
| ----------------------- | -------------------------------------------------------------------------------------------------------- |
| **Direct & group chat** | LAN discovery, verified identities, direct/group/self chats with local history, replies and @ mentions.  |
| **Files & media**       | Local file and image transfer with retry/resume, configurable receive folders and storage quotas.        |
| **Records & privacy**   | Full-history search, encrypted password-protected export/backup, app lock and tray controls.             |
| **Extensibility**       | A tabbed workspace with built-in read-only previews; richer previews and AI bots are planned as plugins. |

## Features

- LAN discovery, contact requests, identity fingerprints, and direct chats; add peers by a known address when needed.
- Direct, group, and self chats with local history, message search, replies and group mentions.
- Local file and image transfers, retry/resume support, configurable receive folders, and attachment storage quotas.
- Chat notifications, tray controls, and a minimize-to-tray option that keeps the app running in the background.
- Encrypted, password-protected chat backups and current-conversation history import/export. Backups do not include identity private keys or attachment files.
- A tabbed workspace with built-in previews for images, PDF, text/log, CSV, JSON, Markdown, script-disabled HTML, and web links. Additional local preview plugins are experimental and read-only; see [plugin notes](docs/PLUGIN.md).
- Simplified Chinese, Traditional Chinese, and English options for the app frame and selected surfaces. Chat screens and some backend messages are not fully translated yet.

Qbird is built with Tauri, Rust, TypeScript, Vite, and SQLite. Peer communication does not use a hosted relay. See [known limitations](docs/PLUGIN.md) for the preview-plugin boundary.

## Vision

Qbird stays deliberately small: the core keeps only what a trustworthy local network messenger needs — identity, discovery, chat, basic file exchange and local data. Everything else is meant to grow through **plugins** rather than by bloating the core.

- **Local-first, no relay.** Your data stays on your machine; there is no account server and no hosted message relay.
- **Plugin ecosystem.** Richer sidebar file previews, AI bots and agent conversations are planned as optional plugins behind a stable, permission-bounded interface. The plugin marketplace is not part of the current scope.
- **Cross-platform.** Windows ships first, with Linux and macOS planned; the architecture is designed for portability and light resource use from the start.
- **Security as a direction, not a claim.** Privacy and safety guide the design, but this preview has not passed an independent security audit and is not presented as production-grade.

## Architecture

Qbird is a Tauri app: a Rust backend owns networking, storage and identity, while a TypeScript frontend renders the UI and talks to the backend through Tauri IPC commands.

```text
+-------------------------------------------------------------+
|  Frontend (TypeScript / Vite)                               |
|  main.ts · chat/group/direct · capture · records · plugins  |
|            |  invoke() / events (Tauri IPC)                 |
|  Backend (Rust)                                             |
|  lib.rs · app_state · listener · discovery · protocol       |
|  storage.rs (SQLite) · backup.rs · identity.rs · files      |
+-------------------------------------------------------------+
```

### Installed data layout

All user data is local. The app stores everything under the per-user data directory `%APPDATA%\im.qbird.desktop\`:

```text
%APPDATA%\im.qbird.desktop\        # app_data_dir (identifier: im.qbird.desktop)
├── qbird.sqlite3                  # chat history, contacts, conversations (NOT encrypted)
├── identity.bin                   # device identity key, protected via Windows DPAPI
├── logs\
│   └── qbird.log                  # rotating app log (+ panic hook)
├── attachments\
│   └── in\
│       ├── direct\                # received files for direct chats
│       └── group\                 # received files for group chats
└── pre-restore-<ts>.db            # transient snapshot created before an overwrite restore
```

> The SQLite database and identity file live only on this machine; there is no hosted relay or account server. Backups are password-encrypted but do not include `identity.bin`.

## Run from source

**Supported build platform:** Windows 10/11 x64. Requirements: Visual Studio 2022 C++ Build Tools with a Windows SDK, Rust (`stable-x86_64-pc-windows-msvc`), Node.js 22+, pnpm 11.22.0, and the WebView2 Runtime.

```bash
git clone https://github.com/TamKingsley/Qbird.git
cd Qbird
pnpm install --frozen-lockfile
pnpm tauri dev
```

## Contributing

Focused pull requests and issue reports are welcome. Please discuss identity, cryptography, or protocol changes in an issue first.

## License

[Apache License 2.0](LICENSE). Third-party dependencies retain their respective licenses.

## Development assistance

Qbird is an open-source, non-commercial project. Its development is assisted by **MuseWork** and **Lingxi** (WPS AI agents).
