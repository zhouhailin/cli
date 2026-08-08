use anyhow::Result;

use crate::core::tools::{java, node};

pub fn run(tool: String) -> Result<()> {
    match tool.as_str() {
        "java" => java::install(None, None),
        "node" => node::install(None),
        _ => Err(anyhow::anyhow!("暂不支持的安装目标: {tool}")),
    }
}
