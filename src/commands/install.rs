use anyhow::{anyhow, Result};

use crate::core::interact::{is_interactive, select};
use crate::core::tools::{go, java, maven, node};

/// 交互列表：中文标签 -> 工具内部名
const TOOL_CHOICES: [(&str, &str); 5] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("自更新", "update"),
];

pub fn run(tool: Option<String>) -> Result<()> {
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
    match tool.as_str() {
        "java" => java::install(None, None),
        "node" => node::install(None),
        "go" => go::install(None),
        "maven" => maven::install(None),
        "update" => crate::core::tools::self_update::run(),
        _ => Err(anyhow!("暂不支持的安装目标: {tool}")),
    }
}
