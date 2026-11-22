# AGENTS.md — Qbird 开发流程与协作规范

本文件是 Qbird 项目面向 AI 编码助手（灵犀/Agent）的开发流程约定。Tam 个人项目，目标是**兼顾规范与敏捷**：本地只跑秒级闸门，重活交给 CI；不可逆/改远端操作先问再做。

## 贯穿铁律

1. **只有代码变动才开 PR**：功能 / 修复 / 配置 / 底层逻辑改动 → 走完整 PR 流程。
2. **纯文档 / README / 注释 / typo 不开 PR**：走轻量通道，直推 main。
3. **PR ≠ 发版**：PR 只是代码变更进主干的通道；只有打 tag 才出新版本。
4. **不可逆/改远端操作先征求 Tam 同意**：合并 PR、推送、打 tag 发版等，动手前先问。
5. **提交信息、PR 标题/描述、CHANGELOG 一律英文**（Conventional Commits）。
6. **多需求自动分解**：一串需求拆成多个分支、多个 PR，一个 PR 只做一件可独立合并的事。

## 工程护栏（已生效）

| 护栏                                        | 作用                                                               |
| ------------------------------------------- | ------------------------------------------------------------------ |
| `.gitattributes`                            | 统一 LF 行尾，杜绝 Windows/Linux 伪 diff                           |
| husky `commit-msg` + commitlint             | 非 Conventional 提交当场拒绝                                       |
| husky `pre-commit` + lint-staged + Prettier | 提交时只格式化本次改动文件（增量，不全仓重排）；Rust 由 rustfmt 管 |
| `pnpm verify`                               | 版本一致性 + 类型检查的秒级快闸门                                  |

## 标准开发流程（GitHub Flow 精简版）

0. **需求分解**：输入可能是 issue / 直接要求 / 一串需求；多需求拆多分支多 PR。
1. **开工**：`git switch main && git pull` 对齐 → `git switch -c feat/xxx` 或 `fix/xxx`。
2. **实现 + 本地诊断（必跑，秒级）**：`pnpm verify`。改 Rust 本地能跑就 `cargo test`；GUI 相关本地跑不了（沙箱 DLL 问题）不强求，靠 CI 兜底。
3. **是否编译一版给 Tam 测**（按风险分级）：
   - 纯逻辑/样式/配置/文档：不必打包。
   - 底层（启动、监听、数据库、备份、打包配置、GUI 交互）：`pnpm tauri build` 出本地版，交 Tam 人工验收。
4. **是否人工测试**：我评估风险，拿不准或属底层类 → 主动给包/问 Tam；低风险 → 告知建议跳过。未通过验收不进入下一步。
5. **才允许 push + 提 PR**：`git push -u origin xxx` → `gh pr create --base main --fill`。
6. **CI 全绿 + Tam review → 合并（先问）**：`gh pr checks` 盯绿；授权后 `gh pr merge --squash --delete-branch` → 本地 `git switch main && git pull`。

## Issue ↔ PR 对应

- 单 issue → 单 PR：描述写 `Closes #N`。
- 多部分 issue → 拆多个 PR：中间用 `Refs #N`，最后一个 `Closes #N`。

## 发版（CD，按需，先问）

1. 整理本次更新/修复内容，写进 `CHANGELOG.md` 对应版本段（英文）。
2. 同步四处版本号：`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` / `src-tauri/Cargo.lock`。
3. `pnpm install` 同步 lockfile（否则 release.yml 的 `--frozen-lockfile` 会失败）。
4. `pnpm verify` 通过 → 提交 `chore: release X.Y.Z` → 打 **annotated tag**（`git tag -a vX.Y.Z`，轻量 tag 不会被 `--follow-tags` 推送）。
5. **推 tag 前先问 Tam** → `git push origin main --follow-tags`（不通则显式 `git push origin vX.Y.Z`）。
6. release.yml 自动：等 main CI 绿 → 构建 NSIS + 便携 ZIP → 发布 GitHub Release（notes 取自 CHANGELOG 对应段）。

## 关键坑（实测）

- **沙箱内 gh/git push 读不到 Windows keyring token**，会误报 401/连不上 → gh、git push 必须**提权**运行。
- **github.com（git push 走它）国内间歇不通**；api.github.com（gh 走它）通常通 → push 失败先重试，长期不通可临时加仓库 deploy key 走 `ssh.github.com:443`（用完删）。
- **gh 便携版**在 `.tools/bin/gh.exe`（已在 .gitignore）；已登录 TamKingsley，scopes: gist/read:org/repo。

## 工具链

- Node ≥ 22、pnpm 11、Rust stable、Tauri 2、Git。
- 构建：`cargo build --release --features custom-protocol`；打包：`tauri bundle --bundles nsis`（`pnpm tauri build` 会 panic）。
- rustc 需设 `RUSTC=<工具链>\stable-x86_64-pc-windows-msvc\bin\rustc.exe`。
