# Changelog

All notable changes to Qbird are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions are tagged `vX.Y.Z`.

Entries are bilingual: each English bullet is followed by its Chinese
translation. Release notes are taken from the matching `## [X.Y.Z]` section.

## [Unreleased]

## [0.1.16] - 2026-11-23

### Added / 新增

- Encrypted chat export: select conversations (direct/group/self) and a time range, then export each as an encrypted `.qchat` file from the chat-records window.
- 加密聊天导出：在聊天记录窗口选择会话（单聊/群聊/自聊）和时间范围，将每个会话导出为加密的 `.qchat` 文件。
- Full-history search in the chat-records window, with search navigation across matching messages.
- 聊天记录窗口支持全历史搜索，可在匹配的消息之间跳转导航。
- Avatars now appear beside chat bubbles in direct and group chats.
- 单聊和群聊的消息气泡旁现在显示头像。

### Fixed / 修复

- Offline message recalls are re-delivered and applied when the peer reconnects, and the original message shows a recall notice; chat avatars stay in sync across peers.
- 离线撤回会在对端重新上线后补发并生效，原消息显示撤回提示；聊天头像在对端之间保持同步。
- LAN contact requests are repaired: presence probing and request handling now complete reliably instead of failing silently.
- 修复局域网联系人请求：在线探测与请求处理现在可靠完成，不再静默失败。
- The chat-records window keeps its window controls usable when a load fails, and surfaces the failure message.
- 聊天记录窗口在加载失败时仍保留可用的窗口控件，并显示失败原因。
- The unfinished "user backup" and "user restore" placeholders are hidden from the More menu in all locales; chat backup/restore behavior is unchanged.
- 在所有语言的“更多”菜单中隐藏尚未完成的“用户备份”和“用户还原”占位项；聊天备份/还原行为不变。

## [0.1.15] - 2026-11-22

### Fixed

- Format-2 backups now restore correctly: `parse_bundle` strips the payload header before handing the snapshot to SQLite, fixing the "file is not a database" failure on restore.
- Fresh installs no longer report "directory missing" for attachment receive folders; they are created at startup.
- Chat records window: close button works (WebviewWindow close) and load failures show a visible message; per-message local delete added in direct/group/self chats via right-click.
- Right-click "hide conversation" now reports success/failure instead of failing silently; text export moved into the chat-records window.
- Listener startup failures are logged with the underlying error and fall back to a random port; last-resort failures are clearly logged.
- Backup/restore/archive button failures surface their reason instead of disappearing.
- Legacy-data migration records its source and shows a one-time notice, so old records on a fresh install are explainable.

### Added

- File logging subsystem (`logs/qbird.log` with rotation and a panic hook) and a log viewer in Debug settings.
- Message timestamps in direct/group/self chats with a "show seconds" setting; the records window honors it.
- Dedicated "Chat Backup & Restore" settings tab; the duplicate archive module was merged into it.
- Export wizard: pick conversations (direct/group/self, select all) and a time range; each selected conversation exports as an encrypted `.qchat` file.
- User backups (format 2) now include chat attachment files (up to 512 MB total) and restore them with database path remapping.
- Address book: contacts renamed, context menus (message / remove contact / leave group) and local contact removal.

## [0.1.14]

- International release preparation, badges, cross-platform names and group controls.
