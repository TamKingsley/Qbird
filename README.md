# Qbird

**A lightweight, local-first LAN messenger.**

English · [Chinese](README_CN.md)

Qbird is an open-source desktop messenger for direct communication on a local network. It is designed to be easy to run without a hosted relay or account service, with data kept on users’ devices.

> **0.1.12-beta · Windows x64 preview.** Qbird has not received an independent security audit. Its local database is not encrypted. Do not use this beta for sensitive information or production communication.

## Preview

![Qbird LAN discovery screen with fictional demo data](docs/assets/qbird-demo.png)

*The contacts and network addresses shown are fictional demo data.*

## Project direction

The foundation is intended to stay lightweight and focused on identity, LAN discovery, messaging, and local data. Optional capabilities should be added as plugins rather than expanding the core. Richer sidebar file-preview providers and AI Bot integrations are roadmap examples, **not shipped features**. The current app includes an experimental local, read-only preview-plugin host; it is not yet a stable general-purpose plugin API.

## Current features

- LAN discovery, contacts, and identity-aware direct messaging.
- Self-chat, group conversations, local history, and file/image transfers.
- Local export and backup tools, plus a tabbed workspace for supported previews.
- An experimental, read-only preview-plugin host. See [plugin notes](docs/PLUGIN.md).

Qbird is built with Tauri, Rust, TypeScript, Vite, and SQLite. Direct peer communication does not use a hosted relay. On Windows, discovery advertises the configured nickname and username to the local network.

## Run from source

**Supported build platform:** Windows 10/11 x64. You will need Visual Studio 2022 C++ Build Tools with a Windows SDK, Rust (`stable-x86_64-pc-windows-msvc`), Node.js 22+, pnpm 11.22.0, and the WebView2 Runtime.

```bash
git clone https://github.com/TamKingsley/Qbird.git
cd Qbird
pnpm install --frozen-lockfile
pnpm tauri dev
```

## Contributing

Qbird is open for community review and collaboration. If you find the direction useful, please **Star** the repository, **Fork** it to experiment, or open an [Issue](https://github.com/TamKingsley/Qbird/issues) to report a problem or suggest a feature. Focused pull requests are welcome; discuss identity, cryptography, or protocol changes in an issue first.

## License

[Apache License 2.0](LICENSE). Third-party dependencies retain their respective licenses.
