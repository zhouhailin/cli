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
        "dragonwell" => vec!["8", "11", "17", "21", "25"],
        "bisheng" => vec!["8", "11", "17", "21"],
        "temurin" | "zulu" | "liberica" => vec!["8", "11", "21"],
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
        "dragonwell" => resolve_dragonwell_url("standard", version, platform),
        "bisheng" => resolve_bisheng_url(version, platform),
        "kona" => {
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

/// 毕昇 JDK 版本 → 鲲鹏下载页 code（JDK8/JDK11/JDK17/B0JDK21）
pub fn bisheng_api_code(version: &str) -> Result<&'static str> {
    match version {
        "8" => Ok("JDK8"),
        "11" => Ok("JDK11"),
        "17" => Ok("JDK17"),
        "21" => Ok("B0JDK21"),
        _ => Err(anyhow!("毕昇 JDK 不支持的版本: {version}")),
    }
}

/// 解析毕昇 JDK 下载信息页 JSON：过滤 JRE，按平台匹配 JDK 包直链
pub fn parse_bisheng_download_url(json: &str, platform: &Platform) -> Result<String> {
    if !matches!(platform.os, crate::core::platform::Os::Linux) {
        return Err(anyhow!(
            "毕昇 JDK 官方仅提供 Linux 构建（AArch64/x86_64），请选择 Temurin/Zulu/Liberica/Kona 等发行版"
        ));
    }
    #[derive(serde::Deserialize)]
    struct Response {
        data: Data,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SoftLink {
        soft_name: String,
        download_link: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        soft_links: Vec<SoftLink>,
    }
    let parsed: Response =
        serde_json::from_str(json).map_err(|e| anyhow!("解析毕昇 JDK 下载信息失败: {e}"))?;
    let arch_key = match platform.arch {
        crate::core::platform::Arch::X86_64 => "linux-x64",
        crate::core::platform::Arch::Aarch64 => "linux-aarch64",
    };
    parsed
        .data
        .soft_links
        .into_iter()
        .filter(|l| l.soft_name.to_lowercase().contains("jdk"))
        .filter(|l| !l.soft_name.to_lowercase().contains("jre"))
        .filter(|l| l.soft_name.contains(arch_key))
        .map(|l| l.download_link)
        .next()
        .ok_or_else(|| anyhow!("毕昇 JDK 未提供 {platform} 构建"))
}

/// 毕昇 JDK 下载 URL：鲲鹏官网下载信息页（需 Referer 校验），华为云镜像直链
pub fn resolve_bisheng_url(version: &str, platform: &Platform) -> Result<String> {
    if !matches!(platform.os, crate::core::platform::Os::Linux) {
        return Err(anyhow!(
            "毕昇 JDK 官方仅提供 Linux 构建（AArch64/x86_64），请选择 Temurin/Zulu/Liberica/Kona 等发行版"
        ));
    }
    let code = bisheng_api_code(version)?;
    let api = format!(
        "https://www.hikunpeng.com/kunpenggateway/kunpengservice/devkit/bsjdk/info/zh/{code}"
    );
    let body = crate::core::download::http_get_string_with_headers(
        &api,
        &[
            ("Referer", "https://www.hikunpeng.com/"),
            ("User-Agent", "Mozilla/5.0"),
        ],
    )
    .map_err(|e| anyhow!("获取毕昇 JDK 发行版信息失败（{e}）"))?;
    parse_bisheng_download_url(&body, platform)
}

/// 抓取毕昇 JDK 包的 SHA-256（华为云镜像提供 {url}.sha256）
pub fn fetch_bisheng_sha256(url: &str) -> Result<String> {
    let text = crate::core::download::http_get_string(&format!("{url}.sha256"))
        .map_err(|e| anyhow!("获取毕昇 JDK SHA-256 失败（{e}）"))?;
    parse_sha256_text(&text)
}

/// 从 sha256 文件文本提取 hash（格式: "<hash>  <文件名>"）
pub fn parse_sha256_text(text: &str) -> Result<String> {
    text.split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("SHA-256 文件格式无效"))
}

/// 解析 Dragonwell 官方 releases.json（https://dragonwell-jdk.io/releases.json）
/// 按渠道（oss/github）× 变体（standard/extended）× 版本查直链
pub fn parse_dragonwell_releases(
    json: &str,
    variant: &str,
    version: &str,
    source: &str,
    platform: &Platform,
) -> Result<String> {
    use std::collections::HashMap;
    #[derive(serde::Deserialize)]
    struct DragonwellSource {
        // 官方 JSON 中部分键为 null（如 extended/17），必须用 Option 承载
        #[serde(default)]
        standard: HashMap<String, Option<String>>,
        #[serde(default)]
        extended: HashMap<String, Option<String>>,
    }
    #[derive(serde::Deserialize)]
    struct DragonwellReleases {
        oss: DragonwellSource,
        github: DragonwellSource,
    }
    let releases: DragonwellReleases = serde_json::from_str(json)
        .map_err(|e| anyhow!("解析 Dragonwell releases.json 失败: {e}"))?;
    if !["standard", "extended"].contains(&variant) {
        return Err(anyhow!("不支持的 Dragonwell 变体: {variant}"));
    }
    if !["oss", "github"].contains(&source) {
        return Err(anyhow!("不支持的 Dragonwell 渠道: {source}"));
    }
    let source_map = if source == "oss" {
        &releases.oss
    } else {
        &releases.github
    };
    let map = if variant == "standard" {
        &source_map.standard
    } else {
        &source_map.extended
    };
    let prefix = match platform.os {
        crate::core::platform::Os::Linux => match platform.arch {
            crate::core::platform::Arch::X86_64 => "xurl",
            crate::core::platform::Arch::Aarch64 => "aurl",
        },
        crate::core::platform::Os::Windows => match platform.arch {
            crate::core::platform::Arch::X86_64 => "wurl",
            crate::core::platform::Arch::Aarch64 => {
                return Err(anyhow!("Dragonwell 官方未提供 Windows (aarch64) 构建"))
            }
        },
        crate::core::platform::Os::MacOs => {
            return Err(anyhow!(
                "Dragonwell 不提供 macOS 构建，请选择 Temurin/Zulu/Liberica/Kona 等发行版"
            ))
        }
    };
    let key = format!("{prefix}{version}");
    map.get(&key)
        .and_then(|u| u.as_ref())
        .filter(|u| !u.is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("Dragonwell {variant} {version} 未提供 {platform} 构建"))
}

/// Dragonwell 下载 URL：官方 releases.json 直链，OSS 优先、GitHub 兜底
pub fn resolve_dragonwell_url(
    variant: &str,
    version: &str,
    platform: &Platform,
) -> Result<String> {
    if matches!(platform.os, crate::core::platform::Os::MacOs) {
        return Err(anyhow!(
            "Dragonwell 不提供 macOS 构建，请选择 Temurin/Zulu/Liberica/Kona 等发行版"
        ));
    }
    let body = crate::core::download::http_get_string("https://dragonwell-jdk.io/releases.json")
        .map_err(|e| anyhow!("获取 Dragonwell 发行版信息失败（{e}）"))?;
    match parse_dragonwell_releases(&body, variant, version, "oss", platform) {
        Ok(url) => Ok(url),
        Err(oss_err) => parse_dragonwell_releases(&body, variant, version, "github", platform)
            .map_err(|gh_err| anyhow!("OSS 与 GitHub 渠道均不可用: OSS({oss_err}) GitHub({gh_err})")),
    }
}

/// GitHub 发行版仓库名（按版本动态选择，TencentKona-8/11/17/21 等独立仓库）
pub fn github_repo(vendor: &str, version: &str) -> Result<String> {
    match vendor {
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
    // Dragonwell 分 Standard/Extended 变体；macOS 官方无构建，提前拦截
    let variant = if vendor.name == "dragonwell" {
        if matches!(Platform::detect().os, crate::core::platform::Os::MacOs) {
            return Err(anyhow!(
                "Dragonwell 不提供 macOS 构建，请选择 Temurin/Zulu/Liberica/Kona 等发行版"
            ));
        }
        let v_idx = select(
            "请选择 Dragonwell 变体",
            &["Standard（标准）", "Extended（增强）"],
        )?;
        if v_idx == 0 {
            "standard"
        } else {
            "extended"
        }
    } else {
        "standard"
    };
    let desc = if vendor.name == "dragonwell" {
        format!("{} Java {version}（{variant}）", vendor.label)
    } else {
        format!("{} Java {version}", vendor.label)
    };
    let platform = Platform::detect();
    let (url, sha256) = if vendor.name == "dragonwell" {
        (resolve_dragonwell_url(variant, &version, &platform)?, None)
    } else if vendor.name == "bisheng" {
        let u = resolve_bisheng_url(&version, &platform)?;
        let s = match fetch_bisheng_sha256(&u) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("警告: {e}，将跳过 SHA-256 校验");
                None
            }
        };
        (u, s)
    } else {
        (resolve_url(vendor.name, &version, &platform)?, None)
    };
    println!("准备安装 {desc}...");
    println!("下载地址: {url}");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let mut ctx = InstallContext::load()?;
    install_archive(&url, sha256.as_deref(), "java", &version, &mut ctx, false)?;
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
        assert_eq!(github_repo("kona", "21").unwrap(), "Tencent/TencentKona-21");
        assert!(github_repo("bisheng", "17").is_err());
        assert!(github_repo("dragonwell", "8").is_err());
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

    #[test]
    fn available_versions_dragonwell_five_versions() {
        let dw = vendors().into_iter().find(|v| v.name == "dragonwell").unwrap();
        assert_eq!(available_versions(&dw), vec!["8", "11", "17", "21", "25"]);
    }

    #[test]
    fn available_versions_bisheng_four_versions() {
        let bs = vendors().into_iter().find(|v| v.name == "bisheng").unwrap();
        assert_eq!(available_versions(&bs), vec!["8", "11", "17", "21"]);
    }

    #[test]
    fn bisheng_api_code_maps_versions() {
        assert_eq!(bisheng_api_code("8").unwrap(), "JDK8");
        assert_eq!(bisheng_api_code("11").unwrap(), "JDK11");
        assert_eq!(bisheng_api_code("17").unwrap(), "JDK17");
        assert_eq!(bisheng_api_code("21").unwrap(), "B0JDK21");
        assert!(bisheng_api_code("25").is_err());
    }

    const BISHENG_SAMPLE: &str = r#"{
      "data": {
        "softLinks": [
          {"softName": "bisheng-jre-8u492-b13-linux-aarch64.tar.gz", "downloadLink": "https://mirrors.huaweicloud.com/kunpeng/archive/compiler/bisheng_jdk/bisheng-jre-8u492-b13-linux-aarch64.tar.gz"},
          {"softName": "bisheng-jdk-8u492-b13-linux-aarch64.tar.gz", "downloadLink": "https://mirrors.huaweicloud.com/kunpeng/archive/compiler/bisheng_jdk/bisheng-jdk-8u492-b13-linux-aarch64.tar.gz"},
          {"softName": "bisheng-jdk-8u492-b13-linux-x64.tar.gz", "downloadLink": "https://mirrors.huaweicloud.com/kunpeng/archive/compiler/bisheng_jdk/bisheng-jdk-8u492-b13-linux-x64.tar.gz"}
        ]
      }
    }"#;

    #[test]
    fn parse_bisheng_download_url_extracts_linux_aarch64() {
        let p = Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::Aarch64,
        };
        // JRE 包被过滤，命中 aarch64 JDK 直链
        assert_eq!(
            parse_bisheng_download_url(BISHENG_SAMPLE, &p).unwrap(),
            "https://mirrors.huaweicloud.com/kunpeng/archive/compiler/bisheng_jdk/bisheng-jdk-8u492-b13-linux-aarch64.tar.gz"
        );
    }

    #[test]
    fn parse_bisheng_download_url_linux_x64() {
        let p = Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::X86_64,
        };
        assert_eq!(
            parse_bisheng_download_url(BISHENG_SAMPLE, &p).unwrap(),
            "https://mirrors.huaweicloud.com/kunpeng/archive/compiler/bisheng_jdk/bisheng-jdk-8u492-b13-linux-x64.tar.gz"
        );
    }

    #[test]
    fn parse_bisheng_download_url_rejects_non_linux() {
        let p = Platform {
            os: crate::core::platform::Os::Windows,
            arch: crate::core::platform::Arch::X86_64,
        };
        let err = parse_bisheng_download_url(BISHENG_SAMPLE, &p).unwrap_err();
        assert!(err.to_string().contains("仅提供 Linux 构建"));
    }

    #[test]
    fn resolve_bisheng_url_rejects_macos() {
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let err = resolve_bisheng_url("21", &p).unwrap_err();
        assert!(err.to_string().contains("仅提供 Linux 构建"));
    }

    #[test]
    fn resolve_bisheng_url_rejects_windows() {
        let p = Platform {
            os: crate::core::platform::Os::Windows,
            arch: crate::core::platform::Arch::X86_64,
        };
        let err = resolve_bisheng_url("17", &p).unwrap_err();
        assert!(err.to_string().contains("仅提供 Linux 构建"));
    }

    #[test]
    fn parse_sha256_text_parses_hash() {
        assert_eq!(
            parse_sha256_text(
                "aabbccddeeff00112233445566778899  bisheng-jdk-8u492-b13-linux-aarch64.tar.gz"
            )
            .unwrap(),
            "aabbccddeeff00112233445566778899"
        );
        assert!(parse_sha256_text("").is_err());
    }

    const DRAGONWELL_SAMPLE: &str = r#"{
      "oss": {
        "standard": {
          "version21": "21.0.6.0.7.6",
          "xurl21": "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_x64_linux.tar.gz",
          "aurl21": "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_aarch64_linux.tar.gz",
          "wurl21": "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_x64_windows.zip"
        },
        "extended": {
          "version21": "21.0.6.0.7.6",
          "aurl21": "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Extended_21.0.6.0.7.6_aarch64_linux.tar.gz"
        }
      },
      "github": {
        "standard": {
          "version21": "21.0.6.0.7.6",
          "aurl21": "https://github.com/dragonwell-project/dragonwell21/releases/download/dragonwell-standard-21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_aarch64_linux.tar.gz"
        }
      }
    }"#;

    fn linux_aarch64() -> Platform {
        Platform {
            os: crate::core::platform::Os::Linux,
            arch: crate::core::platform::Arch::Aarch64,
        }
    }

    #[test]
    fn parse_dragonwell_releases_extracts_oss_url() {
        assert_eq!(
            parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "21", "oss", &linux_aarch64())
                .unwrap(),
            "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_aarch64_linux.tar.gz"
        );
    }

    #[test]
    fn parse_dragonwell_releases_uses_github_source() {
        assert_eq!(
            parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "21", "github", &linux_aarch64())
                .unwrap(),
            "https://github.com/dragonwell-project/dragonwell21/releases/download/dragonwell-standard-21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_aarch64_linux.tar.gz"
        );
    }

    #[test]
    fn parse_dragonwell_releases_extended_variant() {
        assert_eq!(
            parse_dragonwell_releases(DRAGONWELL_SAMPLE, "extended", "21", "oss", &linux_aarch64())
                .unwrap(),
            "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Extended_21.0.6.0.7.6_aarch64_linux.tar.gz"
        );
    }

    #[test]
    fn parse_dragonwell_releases_windows_x64() {
        let p = Platform {
            os: crate::core::platform::Os::Windows,
            arch: crate::core::platform::Arch::X86_64,
        };
        assert_eq!(
            parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "21", "oss", &p).unwrap(),
            "https://dragonwell.oss-cn-shanghai.aliyuncs.com/21.0.6.0.7.6/Alibaba_Dragonwell_Standard_21.0.6.0.7.6_x64_windows.zip"
        );
    }

    #[test]
    fn parse_dragonwell_releases_rejects_macos() {
        let p = Platform {
            os: crate::core::platform::Os::MacOs,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let err = parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "21", "oss", &p)
            .unwrap_err();
        assert!(err.to_string().contains("不提供 macOS 构建"));
    }

    #[test]
    fn parse_dragonwell_releases_rejects_windows_aarch64() {
        let p = Platform {
            os: crate::core::platform::Os::Windows,
            arch: crate::core::platform::Arch::Aarch64,
        };
        let err = parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "21", "oss", &p)
            .unwrap_err();
        assert!(err.to_string().contains("Windows (aarch64)"));
    }

    #[test]
    fn parse_dragonwell_releases_rejects_missing_key() {
        let err = parse_dragonwell_releases(DRAGONWELL_SAMPLE, "standard", "17", "oss", &linux_aarch64())
            .unwrap_err();
        assert!(err.to_string().contains("未提供"));
        let err = parse_dragonwell_releases(DRAGONWELL_SAMPLE, "extended", "21", "github", &linux_aarch64())
            .unwrap_err();
        assert!(err.to_string().contains("未提供"));
    }

    #[test]
    fn parse_dragonwell_releases_rejects_bad_variant() {
        let err = parse_dragonwell_releases(DRAGONWELL_SAMPLE, "pro", "21", "oss", &linux_aarch64())
            .unwrap_err();
        assert!(err.to_string().contains("不支持的 Dragonwell 变体"));
    }
}


