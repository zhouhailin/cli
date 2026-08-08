use anyhow::{anyhow, Result};
use std::path::Path;

use crate::core::config::Config;
use crate::core::interact::select;
use crate::core::links::set_current_link;
use crate::core::paths::DevkitPaths;

pub fn run(tool: String, version: Option<String>) -> Result<()> {
    let paths = DevkitPaths::new()?;
    let mut config = Config::load(&paths)?;
    let installed = config.installed.get(&tool).cloned().unwrap_or_default();
    if installed.is_empty() {
        return Err(anyhow!(
            "{tool} 尚未安装任何版本，请先执行 cli install {tool}"
        ));
    }
    let version = match version {
        Some(v) if installed.contains(&v) => v,
        Some(v) => {
            return Err(anyhow!(
                "版本 {v} 未安装，可用版本: {}",
                installed.join(", ")
            ))
        }
        None => {
            let labels: Vec<String> = installed.iter().map(|v| format!("{tool} {v}")).collect();
            let idx = select("请选择要切换的版本", &labels)?;
            installed[idx].clone()
        }
    };
    // current/<tool> -> ../<tool>/<version>（相对链接，root 目录可整体搬迁）
    let link = paths.current_link(&tool);
    let rel_target = format!("../{tool}/{version}");
    set_current_link(&link, Path::new(&rel_target))?;
    config.set_active(&tool, &version);
    config.save(&paths)?;
    println!("已切换到 {tool} {version}");
    println!("提示: 新终端或 source 当前 shell 配置文件后生效");
    Ok(())
}
