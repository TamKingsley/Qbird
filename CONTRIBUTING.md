# 贡献与开发流程指南（CONTRIBUTING）

> 本文档把 Qbird 的开发流程固化下来。**规范不再只存在于脑子里**——所有约定都已写在这里，并由 Git 钩子和 GitHub Actions 自动校验。
> 这是个人项目，目标是「兼顾规范与敏捷」：本地只跑秒级快闸门，重活（Rust 编译、跨平台测试）交给 PR 上的 CI。

## 0. 一句话流程

> 同步 main → 开 `fix/` 或 `feat/` 分支 → 改代码 → 本地 `pnpm verify` → 你手动验收效果 → `git push` 开 PR → **等 CI 全绿** → 合并 → 删分支。

## 1. 分支模型（精简版 GitHub Flow）

- `main`：唯一长期分支，**始终可发布**。受分支保护，只能通过 PR 合入。
- 所有改动都开短期分支，合完即删：
  - `feat/<简短描述>` — 新功能。例：`feat/screenshot-tray`
  - `fix/<简短描述>` — 修复。例：`fix/listener-port-race`
  - `docs/...`、`chore/...`、`refactor/...`、`ci/...` — 其他类型同理。
- 一个分支只做**一件事**。需求大就拆成多个分支/多个 PR。

### 分支铁律：开工前先和 main 对齐

```bash
git switch main
git pull origin main          # 拉取最新远端 main
git switch -c feat/xxx        # 从最新的 main 切出工作分支
```

## 2. 提交信息规范（由 commitlint 强制）

采用 [Conventional Commits](https://www.conventionalcommits.org/)，格式：

```
<type>: <主题>

[可选正文：说明为什么改、改了哪些]
```

- 允许的 `type`：`feat` `fix` `docs` `style` `refactor` `perf` `test` `build` `ci` `chore` `revert`
- 主题用**小写英文或中文**均可（已放宽大小写检查），尽量一句话说清「做了什么」。
- 示例：
  - `feat: add PixPin-style screenshot capture window`
  - `fix: fall back to random port when listener bind fails`
  - `docs: add contributing guide`
- **写错格式时 `git commit` 会被 `commit-msg` 钩子当场拦下**，不会污染历史。

## 3. 代码风格（由 husky + Prettier 强制）

- 你不需要手动格式化：每次 `git commit` 时，`pre-commit` 钩子会用 Prettier **自动格式化本次改动的文件**并重新加入暂存区。
- Rust 代码由 `rustfmt` 管理，**不在 Prettier 范围内**（见 `.prettierignore`）。
- 行尾统一 LF（见 `.gitattributes`），不会再出现 Windows/Linux 互相打架的伪 diff。
- 想主动全量检查格式：`pnpm format:check`；想主动格式化：`pnpm format`（谨慎，会改动很多文件，单独一个 `chore:` 提交）。

## 4. 本地要跑什么？（关键：别把 CI 的活全揽到本地）

| 场景                             | 命令                                   | 说明                                                                                               |
| -------------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------- |
| **日常每次提交前（必跑，秒级）** | `pnpm verify`                          | = 版本一致性检查 + TypeScript 类型检查。这是你的「快闸门」（格式由提交时钩子自动处理，不在此列）。 |
| 改了 Rust 逻辑                   | `cargo test`（在 `src-tauri/`）        | 本地有 Rust 就跑；嫌慢可以省略，靠 PR 的 CI 兜底。                                                 |
| 要验证打包/桌面效果              | `pnpm tauri build` 或 `pnpm tauri dev` | 由你手动验收功能表现。                                                                             |
| 完整 CI 等价（Rust 编译测试）    | 交给 PR 自动跑                         | **不需要**在本地完整复刻 CI。                                                                      |

> 结论：**你本地优先跑 `pnpm verify` 即可**，编译/跨平台测试由 PR 上的 CI 负责。**是否本地打包一版**取决于改动风险：纯逻辑/样式不必，改到底层（监听、启动、数据库、打包配置）建议你 `pnpm tauri build` 出一个本地版**亲自点一遍**，确认没崩再提 PR。

## 5. 一个完整改动怎么落地（照着做）

```bash
# ① 对齐 main 并切分支
git switch main && git pull origin main
git switch -c feat/your-feature

# ② 写代码……

# ③ 本地快闸门（必须通过）
pnpm verify
# 改了底层就再加一条本地验收：
#   pnpm tauri build   然后亲自运行确认

# ④ 提交（钩子会自动格式化 + 校验提交信息）
git add -A
git commit -m "feat: add your feature"

# ⑤ 推分支并开 PR（装了 gh 后可一条命令）
git push -u origin feat/your-feature
gh pr create --base main --fill        # 自动用最近提交信息作为标题/正文

# ⑥ 等 CI 全绿（本地也能盯着）
gh pr checks --watch

# ⑦ CI 绿 + 你确认无误 → 合并并自动删除本地+远端分支
gh pr merge --squash --delete-branch

# ⑧ 回到 main 并同步
git switch main && git pull origin main
```

## 6. Issue ↔ PR 的对应关系

- **一个 issue 对应一个 PR**：PR 描述里写 `Closes #123`，合并后 issue 自动关闭。
- **一个 issue 涉及多个部分**：
  1. 在 issue 正文用**任务清单**拆成可独立交付的子项：
     ```
     ## 子任务
     - [ ] # 子任务 1（后端监听）
     - [ ] # 子任务 2（前端 UI）
     ```
  2. **每个子项各开一个分支、各提一个 PR**；非最终的 PR 用 `Refs #123`（只关联不关闭），
  3. 最后那个 PR 用 `Closes #123` 收尾，全部完成才关闭 issue。
  4. 子任务可再各自建成独立 issue，用 `#123` 互相 `Blocked by` 引用。

## 7. 发版流程（打 tag → 自动构建发布）

> `release.yml` 已配置：推 `vX.Y.Z` tag → 自动等该 commit 的 CI 绿 → 构建 Windows NSIS + 便携 ZIP → 发布 GitHub Release（release notes 取自 `CHANGELOG.md` 对应版本段）。

```bash
# ① 同步四处版本号到目标版本（如 0.1.15）：
#    package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml / Cargo.lock
#    然后 pnpm verify 会校验它们一致
# ② 更新 CHANGELOG.md，新增 "## [0.1.15]" 段落
# ③ 提交
git commit -am "chore: release 0.1.15"
# ④ 打 tag 并推送
git tag v0.1.15
git push origin main --follow-tags
# ⑤ 到 GitHub Actions 看 Release 工作流跑完，产物出现在 Releases 页
```

> 注：当前 release.yml 把发布标为 `prerelease: true`（预览版）。将来发正式 GA 时把该字段改为 `false`。

## 8. 工具准备（新机器 / 换电脑一次配好）

- Node ≥ 22、pnpm 11、Rust（stable）、Tauri 系统依赖、Git。
- `pnpm install` 会自动触发 `prepare` 钩子装好 husky（无需手动配置钩子）。
- 装 `gh` CLI 并登录：`gh auth login`（用于开 PR / 看 CI / 发版）。

## 9. 已固化的自动化清单

| 关卡         | 位置                                                      | 作用                         |
| ------------ | --------------------------------------------------------- | ---------------------------- |
| 提交前格式化 | husky `pre-commit` + lint-staged                          | 改动文件自动 Prettier        |
| 提交信息校验 | husky `commit-msg` + commitlint                           | 非 Conventional 格式直接拒绝 |
| 版本一致性   | `scripts/check-version.mjs`（含在 `pnpm build`/`verify`） | 四处版本号必须相同，锁 0.1.X |
| 类型检查     | `tsc --noEmit`（含在 `pnpm build`/`verify`）              | TS 编译错误拦截              |

> 注：既有代码尚未全量 Prettier 格式化。格式化采取**增量策略**——提交时 lint-staged 只重排本次改动文件，逐步收敛；不强制一次性全仓重排，避免巨型无关 diff。需要时可手动 `pnpm format`。
> | CI | `.github/workflows/ci.yml`（PR/push 触发） | 前端构建 + Rust 测试(Linux) + Rust 编译(Windows) |
> | 发版闸门 | `.github/workflows/release.yml`（tag 触发） | 强制 CI 绿才发布 + 版本/tag 校验 |
> | 行尾统一 | `.gitattributes` | 杜绝 CRLF/LF 伪 diff |
