use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;
use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};
use crate::core::platform::Platform;

/// mvnd 版本列表页（archive 目录页，仅纯数字稳定版）
const VERSIONS_URL: &str = "https://archive.apache.org/dist/maven/mvnd/";

pub fn install(version_hint: Option<&str>) -> Result<()> {
    let platform = Platform::detect();
    let body = http_get_string(VERSIONS_URL)?;
    let list = crate::core::versions::parse_version_dirs(&body)?;
    let version = if let Some(hint) = version_hint {
        if !list.contains(&hint.to_string()) {
            return Err(anyhow!("版本 {hint} 不可用，请从列表中选择"));
        }
        hint.to_string()
    } else {
        let labels: Vec<String> = list
            .iter()
            .map(|v| format!("Maven Daemon (mvnd) {v}"))
            .collect();
        let idx = select("请选择 mvnd 版本", &labels)?;
        list[idx].clone()
    };
    let url = resolve_url(&version, &platform);
    // archive 提供 .sha256 侧车文件，先取哈希再下载校验
    let sha_text = http_get_string(&format!("{url}.sha256"))?;
    let sha = parse_sha256_text(&sha_text)?;
    println!("准备安装 mvnd {version}...");
    println!("下载地址: {url}");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let mut ctx = InstallContext::load()?;
    install_archive(&url, Some(&sha), "mvnd", &version, &mut ctx, true)?;
    if !ctx.config.active.contains_key("java") {
        println!("提示: mvnd 依赖 Java，请先执行 cli install java");
    }
    println!("mvnd {version} 安装完成");
    Ok(())
}

/// 从 .sha256 文本提取 64 位十六进制哈希（剥离文件名与换行）
pub fn parse_sha256_text(text: &str) -> Result<String> {
    text.split_whitespace()
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("无法从校验和文本解析 SHA-256: {text:?}"))
}

/// mvnd 下载 URL（archive 资产命名：maven-mvnd-<ver>-<os>-<arch>.tar.gz，直接位于版本目录根）
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os {
        crate::core::platform::Os::MacOs => "darwin",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "amd64",
        crate::core::platform::Arch::Aarch64 => "aarch64",
    };
    format!(
        "https://archive.apache.org/dist/maven/mvnd/{version}/maven-mvnd-{version}-{os}-{arch}.tar.gz"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::platform::{Arch, Os};

    #[test]
    fn parse_sha256_text_extracts_hash() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_sha256_text(&format!("{hash}  maven-mvnd-1.0.6-linux-amd64.tar.gz\n")).unwrap(),
            hash
        );
        assert!(parse_sha256_text("short").is_err());
    }

    #[test]
    fn resolve_url_maps_platforms() {
        let linux_x64 = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        assert_eq!(
            resolve_url("1.0.6", &linux_x64),
            "https://archive.apache.org/dist/maven/mvnd/1.0.6/maven-mvnd-1.0.6-linux-amd64.tar.gz"
        );
        let mac_arm = Platform {
            os: Os::MacOs,
            arch: Arch::Aarch64,
        };
        assert_eq!(
            resolve_url("1.0.6", &mac_arm),
            "https://archive.apache.org/dist/maven/mvnd/1.0.6/maven-mvnd-1.0.6-darwin-aarch64.tar.gz"
        );
        let win_x64 = Platform {
            os: Os::Windows,
            arch: Arch::X86_64,
        };
        assert_eq!(
            resolve_url("1.0.6", &win_x64),
            "https://archive.apache.org/dist/maven/mvnd/1.0.6/maven-mvnd-1.0.6-windows-amd64.tar.gz"
        );
    }
}
