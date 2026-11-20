# Qbird

**A local-first messenger for people on the same network.**

English · [简体中文](README_CN.md)

Qbird (provisional Chinese name: **轻书灵雀**) is a desktop messenger designed for direct communication on a local network without a relay server. Windows is the first development target; Linux and macOS remain planned.

> **Development preview · 0.1.11** — This is not a production release. Real two-device, multi-device and current desktop UI acceptance are still incomplete. The local database is not encrypted, and the project has not received an independent security audit. **Do not use it for real communication or sensitive information.**

## Interface preview

![An isolated Qbird desktop development instance with an empty conversation list](docs/assets/qbird-demo.png)

*An isolated, empty development instance from the earlier 0.1.9 interface; 0.1.11 has since changed its onboarding and discovery screens. The application still displays the previous Chinese name; 轻书灵雀 is provisional and has only been introduced in the README.*

## Features

The following are implemented to varying degrees in the development build. Automated tests and real-world acceptance are different; see [the task list](docs/TASKS.md) for details.

| Area | Development preview |
| --- | --- |
| **Getting started and discovery** | First-run onboarding; same-subnet mDNS discovery, manual addresses, rate-limited subnet scanning and a blocklist. Contact addition probes the peer's fingerprint; automatic acceptance can be disabled. |
| **Conversations** | Local self-chat, direct and group chats, hidden conversations, unread counts, search, replies and group mentions. Text and up to nine images can share a message bubble. |
| **Contacts and groups** | Contact profiles, notes and groups; group invitations, member management and join times, group search, join requests and avatars. Invitations require confirmation by default; automatic joining is optional. |
| **Files and images** | Direct and group file transfers with queues, resume and SHA-256 checks; configurable receive directories and confirmation policies; image viewing and a preview workspace. Completed attachments can be opened with the system default app. |
| **Workspace and extensions** | Emoji, local sticker favorites, input preferences, app lock and storage quota. An experimental, local **read-only preview plugin host** is available; viewer plugins are not bundled. |
| **Export and backup** | Plain-text and password-protected chat export/import; password-protected database backup with merge or overwrite restore. Identity keys and attachments are not included in database backups. |

Qbird is built with **Tauri, Rust, TypeScript, Vite and SQLite**. Records are stored locally and peer-to-peer communication does not use an intermediary server.

## Getting started

To run the development build on Windows x64, install:

- WebView2 Runtime and Microsoft C++ Build Tools;
- Rust with the MSVC toolchain and `cargo` on `PATH`;
- Node.js and pnpm (the package manager version is specified in `package.json`).

From the project directory:

```bash
pnpm install --frozen-lockfile
pnpm tauri dev
```

Checks:

```bash
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

`pnpm tauri build` is configured to build an NSIS installer and executable on Windows; generated packages and build outputs are not stored in this repository. For data-directory and distribution details, see [the product requirements](docs/PRD.md).

## Known limitations

- **Verification:** Real two-device, group concurrency, network-failure and current desktop UI acceptance remain incomplete. Successful local or automated tests do not establish production readiness.
- **Offline delivery:** There is no relay. Queued messages and files can be delivered or resumed only when the relevant peers are online.
- **Local data:** SQLite is not encrypted. The optional app lock hides the interface but does not encrypt stored data; Windows authentication reset is not implemented.
- **Backups:** Database backups do not include the identity private key or attachment files. Backup passwords are separate from the app-lock password.
- **Extensions and platforms:** The plugin host is experimental and read-only; editing, a plugin marketplace and bundled document viewers are not provided. Linux and macOS are not yet supported.

## Security and privacy

On Windows, the local identity private key is protected with DPAPI and direct chat uses mutually authenticated TLS. Discovery data and network addresses do not establish identity. Contact notifications are automatically accepted by default in this preview; this can be disabled in settings. Verify full identity fingerprints through an independent, reliable channel before relying on a contact's identity. Transport encryption does not encrypt the local database. Never commit identity keys, chat databases, backups or machine-specific private configuration.

## Project status

See [the task list](docs/TASKS.md) for implementation, test and acceptance status, [the product requirements](docs/PRD.md) for intended scope, and [the plugin API](docs/PLUGIN.md) for the experimental preview interface. Planned capabilities are not shipped features.

## License

[Apache License 2.0](LICENSE). Third-party dependencies retain their respective licenses. Dependency, asset and copyright reviews remain necessary before a public release.
