---
name: release
description: 发布新版本：提交本地变更、更新版本号、打标签 tag、推送远端仓库，自动触发 GitHub Release 构建 5 平台二进制。当用户要求发布、发版、打 tag、推送 release 时使用。
---

# 发布新版本

## 流程

### 1. 检查状态
```bash
git status
```
- 有未提交变更 → 进入步骤 2
- 无变更 → 跳到步骤 3

### 2. 提交本地变更
```bash
git add -A
git commit -m "<描述性提交信息>"
```

### 3. 确认版本号
读取 `Cargo.toml` 中的 `version` 字段，与用户确认目标版本。

- 用户指定版本 → 使用该版本
- 未指定 → 读取最新 tag，按 patch 递增建议下一版本

若 `Cargo.toml` 版本与目标不一致，更新：
```toml
version = "<新版本>"
```
并提交：
```bash
git add Cargo.toml
git commit -m "chore: bump version to <新版本>"
```

### 4. 推送代码
```bash
git push
```
若分支无上游：
```bash
git push --set-upstream origin main
```

### 5. 打标签并推送
```bash
git tag -a v<版本号> -m "Release v<版本号>"
git push origin v<版本号>
```

### 6. 确认触发
标签推送后 GitHub Actions 自动触发 Release 工作流：
- 门禁：`cargo test` + `cargo clippy`
- 构建：5 平台二进制（linux-x64/arm64、macos-x64/arm64、windows-x64）
- 发布：自动创建 GitHub Release 并附加二进制

告知用户 Release 正在构建中，可在 GitHub Actions 页面查看进度。

## 注意事项

- 宪法要求：推送远端前必须确认 `Cargo.toml` 版本号已调整，确保 `cli self-update` 能检测到新版本
- 标签格式：`v` + 语义化版本号，如 `v0.1.17`
- 不要在 main 分支上 force push
