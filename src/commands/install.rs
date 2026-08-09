use anyhow::{anyhow, Result};

use crate::core::cache::{self, CacheManifest};
use crate::core::installer::{install_offline, InstallContext};
use crate::core::interact::{is_interactive, select};
use crate::core::offline;
use crate::core::tools::{go, java, maven, node, rust};

/// 交互列表：中文标签 -> 工具内部名
const TOOL_CHOICES: [(&str, &str); 7] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("Maven Daemon (mvnd)", "mvnd"),
    ("Rust (rustup)", "rust"),
    ("自更新", "update"),
];

/// 离线安装：rust/os 报错；压缩包类从版本清单选择版本后安装
fn offline_install(tool: &str, version_hint: Option<&str>) -> Result<()> {
    if tool == "rust" || tool == "os" {
        return Err(anyhow!(
            "离线模式不支持 {tool} 安装，仅支持 java/node/go/maven/mvnd"
        ));
    }
    let mut ctx = InstallContext::load()?;
    let manifest = cache::load(&ctx.paths.cache_dir())?;
    let versions: Vec<String> = manifest
        .get(tool)
        .map(|list| list.iter().map(|e| e.version.clone()).collect())
        .unwrap_or_default();
    let version = match version_hint {
        // 显式版本直接透传：缓存存在性/校验由 installer 统一处理并给出预热提示
        Some(hint) => hint.to_string(),
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定版本，例如: cli install {tool} <版本>"));
            }
            if versions.is_empty() {
                return Err(anyhow!(
                    "离线模式无 {tool} 可用版本，请先在联网机器执行 cli download {tool} 预热"
                ));
            }
            let idx = select(&format!("请选择要离线安装的 {tool} 版本"), &versions)?;
            versions[idx].clone()
        }
    };
    install_offline(&mut ctx, tool, &version)
}

pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    let paths = crate::core::paths::DevkitPaths::new()?;
    paths.ensure_writable()?;
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli install java"));
            }
            let labels: Vec<&str> = TOOL_CHOICES.iter().map(|(label, _)| *label).collect();
            let idx = select("请选择要安装的工具", &labels)?;
            TOOL_CHOICES[idx].1.to_string()
        }
    };
    if offline::is_offline() {
        println!("离线模式: 仅使用本地缓存，不访问网络");
        return offline_install(&tool, version.as_deref());
    }
    match tool.as_str() {
        "java" => java::install(None, version.as_deref()),
        "node" => node::install(version.as_deref()),
        "go" => go::install(version.as_deref()),
        "maven" => maven::install(version.as_deref()),
        "mvnd" => crate::core::tools::mvnd::install(version.as_deref()),
        "rust" => rust::install(version.as_deref()),
        "update" => crate::core::tools::self_update::run(),
        _ => Err(anyhow!("暂不支持的安装目标: {tool}")),
    }
}
