use anyhow::Result;

use crate::core::config::Config;
use crate::core::paths::DevkitPaths;

pub fn run() -> Result<()> {
    let paths = DevkitPaths::new()?;
    let config = Config::load(&paths)?;
    if config.installed.is_empty() {
        println!("尚未安装任何工具。使用 `cli install <工具>` 开始安装。");
        return Ok(());
    }
    println!("已安装工具:");
    for (tool, versions) in &config.installed {
        let parts: Vec<String> = versions
            .iter()
            .map(|v| {
                if config.active.get(tool) == Some(v) {
                    format!("{v} [当前激活]")
                } else {
                    v.clone()
                }
            })
            .collect();
        println!("  {tool}: {}", parts.join(", "));
    }
    Ok(())
}
