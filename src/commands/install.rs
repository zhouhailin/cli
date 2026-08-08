use anyhow::Result;

use crate::core::tools::{go, java, maven, node};

pub fn run(tool: String) -> Result<()> {
    match tool.as_str() {
        "java" => java::install(None, None),
        "node" => node::install(None),
        "go" => go::install(None),
        "maven" => maven::install(None),
        _ => Err(anyhow::anyhow!("暂不支持的安装目标: {tool}")),
    }
}
