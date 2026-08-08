use anyhow::{anyhow, Result};

use crate::core::download::{download, http_get_string};
use crate::core::interact::confirm;
use crate::core::platform::Platform;
use crate::core::versions::{compare, parse_tag};
use std::cmp::Ordering;

const REPO: &str = "zhouhailin/cli";
const LATEST_API: &str = "https://api.github.com/repos/zhouhailin/cli/releases/latest";

/// 平台 → Release 资产名（与 release.yml 命名一致）
pub fn asset_name(platform: &Platform) -> &'static str {
    match (platform.os, platform.arch) {
        (crate::core::platform::Os::Linux, crate::core::platform::Arch::X86_64) => "cli-linux-x64",
        (crate::core::platform::Os::Linux, crate::core::platform::Arch::Aarch64) => {
            "cli-linux-arm64"
        }
        (crate::core::platform::Os::MacOs, crate::core::platform::Arch::X86_64) => "cli-macos-x64",
        (crate::core::platform::Os::MacOs, crate::core::platform::Arch::Aarch64) => {
            "cli-macos-arm64"
        }
        (crate::core::platform::Os::Windows, crate::core::platform::Arch::X86_64) => {
            "cli-windows-x64.exe"
        }
        (crate::core::platform::Os::Windows, crate::core::platform::Arch::Aarch64) => {
            "cli-windows-arm64.exe"
        }
    }
}

/// 从 GitHub latest release API 响应提取 tag_name
pub fn parse_latest_release(json: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct LatestRelease {
        tag_name: Option<String>,
    }
    let parsed: LatestRelease =
        serde_json::from_str(json).map_err(|e| anyhow!("解析最新版本信息失败: {e}"))?;
    parsed
        .tag_name
        .ok_or_else(|| anyhow!("最新版本响应缺少 tag_name"))
}

pub fn run() -> Result<()> {
    let current = crate::current_version();
    let body = http_get_string(LATEST_API).map_err(|e| anyhow!("检查更新失败（{e}）"))?;
    let latest_tag = parse_latest_release(&body)?;
    let latest = parse_tag(&latest_tag);
    if compare(latest, current) != Ordering::Greater {
        println!("已是最新版本 ({current})");
        return Ok(());
    }
    println!("当前版本: {current} → 最新版本: {latest}");
    if !confirm("确认下载更新？", true)? {
        println!("已取消");
        return Ok(());
    }
    let platform = Platform::detect();
    let asset = asset_name(&platform);
    let url = format!("https://github.com/{REPO}/releases/download/{latest_tag}/{asset}");
    println!("下载地址: {url}");
    let exe = std::env::current_exe()?;
    let staging = exe.with_extension("update");
    download(&url, &staging, None, "cli 自更新")?;
    // Unix 直接原子替换；Windows 运行中 exe 被锁，提示手动替换
    #[cfg(not(windows))]
    {
        replace_binary(&staging, &exe)?;
        println!("更新完成，当前版本: {latest}");
    }
    #[cfg(windows)]
    {
        let new_exe = exe.with_extension("new.exe");
        std::fs::rename(&staging, &new_exe)?;
        println!(
            "已下载新版到 {}，请手动替换当前 cli.exe 后重新运行",
            new_exe.display()
        );
    }
    Ok(())
}

/// Unix 原子替换：staging 权限继承原 exe；原 exe 无执行位或不可读时 0755 兜底，替换后自检补位
/// （HTTP 下载默认 0644，直接 rename 会丢失执行位导致更新后无法运行）
#[cfg(not(windows))]
pub fn replace_binary(staging: &std::path::Path, exe: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match std::fs::metadata(exe) {
        Ok(meta) => {
            let bits = meta.permissions().mode() & 0o777;
            if bits & 0o111 == 0 {
                0o755
            } else {
                bits
            }
        }
        Err(_) => 0o755,
    };
    std::fs::set_permissions(staging, std::fs::Permissions::from_mode(mode))
        .map_err(|e| anyhow!("设置临时文件权限失败: {e}"))?;
    std::fs::rename(staging, exe).map_err(|e| anyhow!("替换二进制失败: {e}"))?;
    // 替换后自检：仍无执行位则补（防御边界场景）
    let after = std::fs::metadata(exe)?.permissions().mode() & 0o777;
    if after & 0o111 == 0 {
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(after | 0o111))?;
    }
    crate::debug_log!("更新后二进制权限: {mode:o}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::platform::{Arch, Os};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn asset_name_maps_all_platforms() {
        let cases = [
            ((Os::Linux, Arch::X86_64), "cli-linux-x64"),
            ((Os::Linux, Arch::Aarch64), "cli-linux-arm64"),
            ((Os::MacOs, Arch::X86_64), "cli-macos-x64"),
            ((Os::MacOs, Arch::Aarch64), "cli-macos-arm64"),
            ((Os::Windows, Arch::X86_64), "cli-windows-x64.exe"),
        ];
        for ((os, arch), expected) in cases {
            let p = Platform { os, arch };
            assert_eq!(asset_name(&p), expected);
        }
    }

    #[test]
    fn parse_latest_release_extracts_tag() {
        let json = r#"{"tag_name": "v0.1.1", "name": "v0.1.1"}"#;
        assert_eq!(parse_latest_release(json).unwrap(), "v0.1.1");
        assert!(parse_latest_release("{}").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_preserves_execute_permission() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        let staging = dir.path().join("cli.update");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&staging, b"new").unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o644)).unwrap();
        replace_binary(&staging, &exe).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_preserves_custom_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        let staging = dir.path().join("cli.update");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&staging, b"new").unwrap();
        replace_binary(&staging, &exe).unwrap();
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn replace_binary_ensures_execute_when_original_missing_exec() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        let staging = dir.path().join("cli.update");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&staging, b"new").unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o644)).unwrap();
        replace_binary(&staging, &exe).unwrap();
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }
}
