use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;
use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};
use crate::core::platform::Platform;
use crate::core::shell::{inject_path, rc_file_for_shell};

#[derive(Debug)]
pub struct NodeLts {
    pub version: String,
    pub codename: String,
}

/// 解析 nodejs.org/dist/index.json，过滤 LTS 行，每个大版本保留版本号最高的一条
pub fn parse_node_lts(json: &str) -> Result<Vec<NodeLts>> {
    #[derive(serde::Deserialize)]
    struct NodeEntry {
        version: String,
        #[serde(default)]
        lts: serde_json::Value,
    }
    let entries: Vec<NodeEntry> =
        serde_json::from_str(json).map_err(|e| anyhow!("解析 Node 版本列表失败: {e}"))?;
    let mut list: Vec<NodeLts> = Vec::new();
    for e in entries {
        if e.lts.is_boolean() && !e.lts.as_bool().unwrap_or(false) {
            continue; // lts: false
        }
        let codename = if e.lts.is_string() {
            e.lts.as_str().unwrap_or("").to_string()
        } else {
            String::new()
        };
        list.push(NodeLts { version: e.version, codename });
    }
    // 版本降序（数字分段比较，不依赖原始顺序）
    list.sort_by(|a, b| crate::core::versions::compare(&b.version, &a.version));
    // 大版本去重：保留版本号最高的一条
    let mut result: Vec<NodeLts> = Vec::new();
    for n in list {
        let major = n
            .version
            .trim_start_matches('v')
            .split('.')
            .next()
            .unwrap_or("")
            .to_string();
        if result
            .iter()
            .any(|r| r.version.trim_start_matches('v').starts_with(&format!("{major}.")))
        {
            continue;
        }
        result.push(n);
    }
    Ok(result)
}

pub fn fetch_lts_list() -> Result<Vec<NodeLts>> {
    let body = http_get_string("https://nodejs.org/dist/index.json")?;
    parse_node_lts(&body)
}

/// node 下载 URL：https://nodejs.org/dist/<version>/node-<version>-<os>-<arch>.<ext>
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let (os, ext) = match platform.os {
        crate::core::platform::Os::MacOs => ("darwin", "tar.gz"),
        crate::core::platform::Os::Linux => ("linux", "tar.gz"),
        crate::core::platform::Os::Windows => ("win", "zip"),
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "x64",
        crate::core::platform::Arch::Aarch64 => "arm64",
    };
    format!("https://nodejs.org/dist/{version}/node-{version}-{os}-{arch}.{ext}")
}

pub fn install(version_hint: Option<&str>) -> Result<()> {
    let list = fetch_lts_list()?;
    let version = if let Some(hint) = version_hint {
        hint.to_string()
    } else {
        let labels: Vec<String> = list
            .iter()
            .map(|n| format!("{}（LTS: {}）", n.version, n.codename))
            .collect();
        let idx = select("请选择 Node.js LTS 版本", &labels)?;
        list[idx].version.clone()
    };
    println!("准备安装 Node.js {version}...");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let platform = Platform::detect();
    let url = resolve_url(&version, &platform);
    let mut ctx = InstallContext::load()?;
    install_archive(&url, None, "node", &version, &mut ctx, false)?;
    let rc_file = rc_file_for_shell()?;
    inject_path(&rc_file, &ctx.paths.current_link("node").join("bin"))?;
    println!("Node.js {version} 安装完成");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_lts_filters_and_dedupes() {
        let json = r#"[
          {"version":"v20.19.0","lts":"Iron"},
          {"version":"v20.18.3","lts":"Iron"},
          {"version":"v22.11.0","lts":"Jod"},
          {"version":"v22.12.0","lts":"Jod"},
          {"version":"v23.0.0","lts":false},
          {"version":"v18.20.4","lts":"Hydrogen"}
        ]"#;
        let list = parse_node_lts(json).unwrap();
        // 大版本 22/20/18 各一条（组内取版本最高），lts=false 的过滤
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].version, "v22.12.0"); // 虽后出现但版本更高
        assert_eq!(list[0].codename, "Jod");
        assert_eq!(list[1].version, "v20.19.0");
        assert_eq!(list[1].codename, "Iron");
        assert_eq!(list[2].version, "v18.20.4");
    }

    #[test]
    fn parse_node_lts_rejects_invalid_json() {
        let err = parse_node_lts("not-json").unwrap_err();
        assert!(err.to_string().contains("解析"));
    }

    #[test]
    fn resolve_url_macos_arm64() {
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        assert_eq!(
            resolve_url("v22.12.0", &p),
            "https://nodejs.org/dist/v22.12.0/node-v22.12.0-darwin-arm64.tar.gz"
        );
    }

    #[test]
    fn resolve_url_windows_x64() {
        let p = Platform {
            os: crate::core::platform::Os::Windows,
            arch: crate::core::platform::Arch::X86_64,
        };
        assert_eq!(
            resolve_url("v20.19.0", &p),
            "https://nodejs.org/dist/v20.19.0/node-v20.19.0-win-x64.zip"
        );
    }
}
