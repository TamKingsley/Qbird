# Changelog

All notable changes to Qbird are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions are tagged `vX.Y.Z`.

## [Unreleased]

### Fixed
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
