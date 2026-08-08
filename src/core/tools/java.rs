use anyhow::{anyhow, Result};

use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};
use crate::core::platform::Platform;
use crate::core::shell::{inject_env_var, inject_path, rc_file_for_shell};

pub struct Vendor {
    pub name: &'static str,
    pub label: &'static str,
}

pub fn vendors() -> Vec<Vendor> {
    vec![
        Vendor { name: "dragonwell", label: "Dragonwell（阿里）" },
        Vendor { name: "bisheng", label: "Bisheng 毕昇（华为）" },
        Vendor { name: "temurin", label: "Temurin（Eclipse Adoptium）" },
        Vendor { name: "zulu", label: "Zulu（Azul）" },
        Vendor { name: "liberica", label: "Liberica（BellSoft）" },
        Vendor { name: "kona", label: "Kona（腾讯）" },
    ]
}

/// 各发行版支持的 Java 版本
pub fn available_versions(vendor: &Vendor) -> Vec<&'static str> {
    match vendor.name {
        "dragonwell" | "bisheng" | "temurin" | "zulu" | "liberica" => vec!["8", "11", "21"],
        "kona" => vec!["8", "11", "17", "21"],
        _ => vec![],
    }
}

/// 解析 Temurin 下载 URL（Adoptium API 直接定位二进制）
pub fn resolve_temurin_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os {
        crate::core::platform::Os::MacOs => "mac",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "x64",
        crate::core::platform::Arch::Aarch64 => "aarch64",
    };
    format!(
        "https://api.adoptium.net/v3/binary/latest/{version}/ga/{os}/{arch}/jdk/hotspot/normal/eclipse?project=jdk"
    )
}

/// 从 Azul API JSON 中提取下载 URL（测试提供样例 JSON）
pub fn parse_azul_download_url(json: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct AzulResponse {
        package_url: Option<String>,
    }
    let parsed: AzulResponse =
        serde_json::from_str(json).map_err(|e| anyhow!("解析 Azul API 响应失败: {e}"))?;
    parsed
        .package_url
        .ok_or_else(|| anyhow!("Azul API 响应缺少 package_url"))
}

/// 解析发行版下载 URL；Temurin 直接生成，其余通过各自 API（Zulu/BellSoft/GitHub Releases）
pub fn resolve_url(vendor: &str, version: &str, platform: &Platform) -> Result<String> {
    match vendor {
        "temurin" => Ok(resolve_temurin_url(version, platform)),
        "zulu" => {
            let os = match platform.os {
                crate::core::platform::Os::MacOs => "macos",
                crate::core::platform::Os::Linux => "linux",
                crate::core::platform::Os::Windows => "windows",
            };
            let arch = match platform.arch {
                crate::core::platform::Arch::X86_64 => "x86_64",
                crate::core::platform::Arch::Aarch64 => "aarch64",
            };
            let api = format!(
                "https://api.azul.com/metadata/v1.1/zulu/packages/?java_version={version}&os={os}&arch={arch}&archive_type=tar.gz&java_package_type=jdk&latest=true&release_status=ga&availability_types=CA"
            );
            let body = crate::core::download::http_get_string(&api)?;
            parse_azul_download_url(&body)
        }
        "liberica" => {
            let os = match platform.os {
                crate::core::platform::Os::MacOs => "macos",
                crate::core::platform::Os::Linux => "linux",
                crate::core::platform::Os::Windows => "windows",
            };
            let arch = match platform.arch {
                crate::core::platform::Arch::X86_64 => "x86_64",
                crate::core::platform::Arch::Aarch64 => "aarch64",
            };
            let api = format!(
                "https://api.bell-sw.com/v1/liberica/releases?version-feature={version}&os={os}&arch={arch}&package-type=tar.gz&bitness=64&release-type=all"
            );
            let body = crate::core::download::http_get_string(&api)?;
            parse_liberica_download_url(&body)
        }
        "dragonwell" | "bisheng" | "kona" => {
            // Dragonwell 官方仅发布 Linux/Windows 构建
            if vendor == "dragonwell"
                && matches!(platform.os, crate::core::platform::Os::MacOs)
            {
                return Err(anyhow!(
                    "Dragonwell 不提供 macOS 构建，请选择 Temurin/Zulu/Liberica/Kona 等发行版"
                ));
            }
            let repo = github_repo(vendor, version)?;
            let api = format!("https://api.github.com/repos/{repo}/releases/latest");
            let body = crate::core::download::http_get_string(&api).map_err(|e| {
                anyhow!("获取 {vendor} 发行版信息失败（{e}），官方可能未发布该版本或渠道不可用")
            })?;
            parse_github_release_url(&body, platform)
        }
        _ => Err(anyhow!("不支持的发行版: {vendor}")),
    }
}

/// GitHub 发行版仓库名（按版本动态选择，dragonwell8/11/17/21 等独立仓库）
pub fn github_repo(vendor: &str, version: &str) -> Result<String> {
    match vendor {
        "dragonwell" => Ok(format!("dragonwell-project/dragonwell{version}")),
        "bisheng" => Ok(format!("openeuler/bishengjdk-{version}")),
        "kona" => Ok(format!("Tencent/TencentKona-{version}")),
        _ => Err(anyhow!("不支持的发行版: {vendor}")),
    }
}

/// 从 BellSoft API JSON 提取首个 downloadUrl
pub fn parse_liberica_download_url(json: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct LibericaItem {
        download_url: Option<String>,
    }
    let parsed: Vec<LibericaItem> =
        serde_json::from_str(json).map_err(|e| anyhow!("解析 Liberica API 响应失败: {e}"))?;
    parsed
        .into_iter()
        .find_map(|i| i.download_url)
        .ok_or_else(|| anyhow!("Liberica API 响应中未找到下载地址"))
}

/// 从 GitHub Releases JSON 中匹配当前平台/架构的 JDK 资产下载地址
pub fn parse_github_release_url(json: &str, platform: &Platform) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(serde::Deserialize)]
    struct Release {
        assets: Vec<Asset>,
    }
    let release: Release =
        serde_json::from_str(json).map_err(|e| anyhow!("解析 GitHub Releases 响应失败: {e}"))?;
    let os_key = match platform.os {
        crate::core::platform::Os::MacOs => "macos",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch_key = match platform.arch {
        crate::core::platform::Arch::X86_64 => "x64",
        crate::core::platform::Arch::Aarch64 => "aarch64",
    };
    release
        .assets
        .into_iter()
        .map(|a| (a.name, a.browser_download_url))
        .filter(|(name, _)| {
            let name_lc = name.to_lowercase();
            let looks_jdk = ["jdk", "java", "dragonwell", "bisheng", "kona"]
                .iter()
                .any(|k| name_lc.contains(k));
            looks_jdk
                && name_lc.contains(os_key)
                && name_lc.contains(arch_key)
                && (name.ends_with(".tar.gz") || name.ends_with(".zip"))
        })
        .map(|(_, url)| url)
        .next()
        .ok_or_else(|| {
            anyhow!("该发行版未提供 {platform} 的 JDK 构建，请选择其他发行版或版本")
        })
}

/// 交互式安装：选发行版 → 选版本 → 下载安装 → JAVA_HOME/PATH 注入
pub fn install(vendor_hint: Option<&str>, version_hint: Option<&str>) -> Result<()> {
    let vendor_list = vendors();
    let vendor_idx = if let Some(hint) = vendor_hint {
        vendor_list
            .iter()
            .position(|v| v.name == hint)
            .ok_or_else(|| anyhow!("不支持的发行版: {hint}"))?
    } else {
        let labels: Vec<String> = vendor_list.iter().map(|v| v.label.to_string()).collect();
        select("请选择 JDK 发行版", &labels)?
    };
    let vendor = &vendor_list[vendor_idx];
    let versions = available_versions(vendor);
    let version = if let Some(hint) = version_hint {
        if !versions.contains(&hint) {
            return Err(anyhow!(
                "{hint} 不支持发行版 {}，可用版本: {}",
                vendor.name,
                versions.join("/")
            ));
        }
        hint.to_string()
    } else {
        let version_labels: Vec<String> = versions.iter().map(|v| format!("Java {v}")).collect();
        let v_idx = select("请选择 Java 版本", &version_labels)?;
        versions[v_idx].to_string()
    };
    println!("准备安装 {} Java {version}...", vendor.label);
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let platform = Platform::detect();
    let url = resolve_url(vendor.name, &version, &platform)?;
    let mut ctx = InstallContext::load()?;
    install_archive(&url, None, "java", &version, &mut ctx, false)?;
    // JAVA_HOME 注入（指向 current 链）
    let rc_file = rc_file_for_shell()?;
    let home = std::env::var("HOME").unwrap_or_default();
    let java_home = if home.is_empty() {
        ctx.paths.current_link("java").to_string_lossy().to_string()
    } else {
        format!("{home}/.devkit/current/java")
    };
    inject_env_var(&rc_file, "JAVA_HOME", &java_home)?;
    inject_path(&rc_file, &ctx.paths.current_link("java").join("bin"))?;
    println!("Java {version}（{}）安装完成", vendor.label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendors_contains_six_entries() {
        assert_eq!(vendors().len(), 6);
    }

    #[test]
    fn resolve_temurin_url_macos_aarch64() {
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let url = resolve_temurin_url("21", &p);
        assert_eq!(
            url,
            "https://api.adoptium.net/v3/binary/latest/21/ga/mac/aarch64/jdk/hotspot/normal/eclipse?project=jdk"
        );
    }

    #[test]
    fn resolve_temurin_url_linux_x64() {
        let p = Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::X86_64,
        };
        let url = resolve_temurin_url("8", &p);
        assert_eq!(
            url,
            "https://api.adoptium.net/v3/binary/latest/8/ga/linux/x64/jdk/hotspot/normal/eclipse?project=jdk"
        );
    }

    #[test]
    fn parse_azul_download_url_extracts_package_url() {
        let json = r#"{
          "package_url": "https://cdn.azul.com/zulu/bin/zulu17.52.17-ca-jdk17.0.12-macosx_aarch64.tar.gz"
        }"#;
        assert_eq!(
            parse_azul_download_url(json).unwrap(),
            "https://cdn.azul.com/zulu/bin/zulu17.52.17-ca-jdk17.0.12-macosx_aarch64.tar.gz"
        );
    }

    #[test]
    fn parse_azul_download_url_rejects_empty() {
        let err = parse_azul_download_url("{}").unwrap_err();
        assert!(err.to_string().contains("package_url"));
    }

    #[test]
    fn parse_liberica_download_url_extracts_first_url() {
        let json = r#"[
          {"download_url":"https://download.bell-sw.com/java/21.0.4+9/bellsoft-jdk21.0.4+9-macos-aarch64.tar.gz"},
          {"download_url":"https://download.bell-sw.com/java/21.0.3/bellsoft-jdk21.0.3-macos-aarch64.tar.gz"}
        ]"#;
        assert_eq!(
            parse_liberica_download_url(json).unwrap(),
            "https://download.bell-sw.com/java/21.0.4+9/bellsoft-jdk21.0.4+9-macos-aarch64.tar.gz"
        );
    }

    #[test]
    fn parse_github_release_url_matches_platform_asset() {
        let json = r#"{
          "assets": [
            {"name":"Dragonwell21_x64_linux.tar.gz","browser_download_url":"https://github.com/gh/dragonwell-linux.tar.gz"},
            {"name":"dragonwell21_aarch64_macos.tar.gz","browser_download_url":"https://github.com/gh/dragonwell-macos.tar.gz"}
          ]
        }"#;
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        assert_eq!(
            parse_github_release_url(json, &p).unwrap(),
            "https://github.com/gh/dragonwell-macos.tar.gz"
        );
    }

    #[test]
    fn parse_github_release_url_rejects_no_match() {
        let json = r#"{"assets":[{"name":"docs.pdf","browser_download_url":"https://x/y.pdf"}]}"#;
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let err = parse_github_release_url(json, &p).unwrap_err();
        assert!(err.to_string().contains("未提供 macos (aarch64)"));
    }

    #[test]
    fn github_repo_maps_by_version() {
        assert_eq!(
            github_repo("dragonwell", "8").unwrap(),
            "dragonwell-project/dragonwell8"
        );
        assert_eq!(github_repo("kona", "21").unwrap(), "Tencent/TencentKona-21");
        assert_eq!(github_repo("bisheng", "17").unwrap(), "openeuler/bishengjdk-17");
        assert!(github_repo("unknown", "8").is_err());
    }

    #[test]
    fn resolve_url_rejects_dragonwell_on_macos() {
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let err = resolve_url("dragonwell", "21", &p).unwrap_err();
        assert!(err.to_string().contains("不提供 macOS 构建"));
    }

    #[test]
    fn resolve_url_uses_temurin_direct() {
        let p = Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::X86_64,
        };
        let url = resolve_url("temurin", "21", &p).unwrap();
        assert!(url.starts_with("https://api.adoptium.net/"));
    }

    #[test]
    fn resolve_url_rejects_unknown_vendor() {
        let p = Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::X86_64,
        };
        let err = resolve_url("unknown", "21", &p).unwrap_err();
        assert!(err.to_string().contains("不支持的发行版"));
    }

    #[test]
    fn available_versions_include_kona_17() {
        let kona = vendors().into_iter().find(|v| v.name == "kona").unwrap();
        assert_eq!(available_versions(&kona), vec!["8", "11", "17", "21"]);
    }
}
