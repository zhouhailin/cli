use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;
use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};
use crate::core::platform::Platform;
use crate::core::shell::{inject_path, rc_file_for_shell};

/// 解析 go.p2hp.com/go.dev/dl/?mode=json：stable 且含当前平台文件，版本降序（最新在前）
pub fn parse_go_versions(json: &str, platform: &Platform) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct GoRelease {
        version: String,
        stable: bool,
        files: Vec<GoFile>,
    }
    #[derive(serde::Deserialize)]
    struct GoFile {
        filename: String,
    }
    let releases: Vec<GoRelease> =
        serde_json::from_str(json).map_err(|e| anyhow!("解析 Go 版本列表失败: {e}"))?;
    let os_key = match platform.os {
        crate::core::platform::Os::MacOs => "darwin",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch_key = match platform.arch {
        crate::core::platform::Arch::X86_64 => "amd64",
        crate::core::platform::Arch::Aarch64 => "arm64",
    };
    let mut versions: Vec<String> = releases
        .iter()
        .filter(|r| r.stable)
        .filter(|r| {
            r.files
                .iter()
                .any(|f| f.filename.contains(os_key) && f.filename.contains(arch_key))
        })
        .map(|r| r.version.trim_start_matches("go").to_string())
        .collect();
    versions.sort_by(|a, b| compare_versions(b, a));
    Ok(versions)
}

/// 版本号比较（点分段数字），供 maven 等模块复用
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    crate::core::versions::compare(a, b)
}

pub fn fetch_versions(platform: &Platform) -> Result<Vec<String>> {
    let body = http_get_string("https://go.p2hp.com/go.dev/dl/?mode=json")?;
    parse_go_versions(&body, platform)
}

/// go 下载 URL：阿里云镜像（国内加速）https://mirrors.aliyun.com/golang/go<version>.<os>-<arch>.tar.gz
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os {
        crate::core::platform::Os::MacOs => "darwin",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "amd64",
        crate::core::platform::Arch::Aarch64 => "arm64",
    };
    format!("https://mirrors.aliyun.com/golang/go{version}.{os}-{arch}.tar.gz")
}

pub fn install(version_hint: Option<&str>) -> Result<()> {
    let platform = Platform::detect();
    let list = fetch_versions(&platform)?;
    let version = if let Some(hint) = version_hint {
        if !list.contains(&hint.to_string()) {
            return Err(anyhow!("版本 {hint} 不可用，请从列表中选择"));
        }
        hint.to_string()
    } else {
        let labels: Vec<String> = list.iter().map(|v| format!("Go {v}")).collect();
        let idx = select("请选择 Go 版本", &labels)?;
        list[idx].clone()
    };
    let url = resolve_url(&version, &platform);
    println!("准备安装 Go {version}...");
    println!("下载地址: {url}");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let mut ctx = InstallContext::load()?;
    install_archive(&url, None, "go", &version, &mut ctx, false)?;
    // 注入 GOPROXY 国内镜像（失败仅警告）
    let go_bin = ctx.paths.tool_dir("go", &version).join("bin").join("go");
    match std::process::Command::new(&go_bin)
        .args(["env", "-w", "GOPROXY=https://goproxy.cn,direct"])
        .status()
    {
        Ok(s) if s.success() => println!("已配置 GOPROXY=https://goproxy.cn,direct"),
        _ => eprintln!(
            "警告: 配置 GOPROXY 失败，可手动执行 go env -w GOPROXY=https://goproxy.cn,direct"
        ),
    }
    let rc_file = rc_file_for_shell()?;
    inject_path(&rc_file, &ctx.paths.current_link("go").join("bin"))?;
    crate::core::shell::print_activation_hint()?;
    println!("Go {version} 安装完成");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac_arm() -> Platform {
        Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        }
    }

    #[test]
    fn parse_go_versions_filters_and_sorts_desc() {
        let json = r#"[
          {"version":"go1.23.0","stable":true,"files":[{"filename":"go1.23.0.darwin-arm64.tar.gz"}]},
          {"version":"go1.22.6","stable":true,"files":[{"filename":"go1.22.6.linux-amd64.tar.gz"}]},
          {"version":"go1.22.5","stable":false,"files":[{"filename":"go1.22.5.darwin-arm64.tar.gz"}]},
          {"version":"go1.21.13","stable":true,"files":[{"filename":"go1.21.13.darwin-arm64.tar.gz"}]},
          {"version":"go1.21.9","stable":true,"files":[{"filename":"go1.21.9.darwin-arm64.tar.gz"}]}
        ]"#;
        let list = parse_go_versions(json, &mac_arm()).unwrap();
        // 1.22.6 无 mac 文件被过滤；1.22.5 非 stable 被过滤；降序
        assert_eq!(list, vec!["1.23.0", "1.21.13", "1.21.9"]);
    }

    #[test]
    fn parse_go_versions_rejects_invalid_json() {
        let err = parse_go_versions("nope", &mac_arm()).unwrap_err();
        assert!(err.to_string().contains("解析"));
    }

    #[test]
    fn resolve_url_macos_arm64() {
        assert_eq!(
            resolve_url("1.22.6", &mac_arm()),
            "https://mirrors.aliyun.com/golang/go1.22.6.darwin-arm64.tar.gz"
        );
    }
}
