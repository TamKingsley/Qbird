# Qbird

**A local-first messenger for direct communication over a local network.**

English · [简体中文](README_CN.md)

Qbird is a Windows desktop app for discovering nearby Qbird users and communicating directly between devices. It has no hosted relay or account service; chat history and identity data are stored locally.

> **0.1.13 · Windows x64 preview.** Qbird has not received an independent security audit. The local SQLite database is not encrypted, and App Lock only hides the interface. Do not use this preview for sensitive information or production communication. LAN discovery advertises the nickname and username configured in the app.

## Preview

![Qbird 0.1.13 message screen](docs/assets/qbird-demo.png)

*The screenshot shows the current build with an isolated local profile and no personal data.*

## Features

- LAN discovery, contact requests, identity fingerprints, and direct chats; add peers by a known address when needed.
- Direct, group, and self chats with local history, message search, replies and group mentions.
- Local file and image transfers, retry/resume support, configurable receive folders, and attachment storage quotas.
- Chat notifications, tray controls, and a minimize-to-tray option that keeps the app running in the background.
- Encrypted, password-protected chat backups and current-conversation history import/export. Backups do not include identity private keys or attachment files.
- A tabbed workspace with built-in previews for images, PDF, text/log, CSV, JSON, Markdown, script-disabled HTML, and web links. Additional local preview plugins are experimental and read-only; see [plugin notes](docs/PLUGIN.md).
- Simplified Chinese, Traditional Chinese, and English options for the app frame and selected surfaces. Chat screens and some backend messages are not fully translated yet.

Qbird is built with Tauri, Rust, TypeScript, Vite, and SQLite. Peer communication does not use a hosted relay. See [known limitations](docs/PLUGIN.md) for the preview-plugin boundary.

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
