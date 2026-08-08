use anyhow::Result;

use crate::core::paths::DevkitPaths;
use crate::core::platform::Platform;

pub fn run() -> Result<()> {
    let platform = Platform::detect();
    let paths = DevkitPaths::new()?;
    println!("cli {}", crate::current_version());
    println!("平台: {} ({})", platform.os_name(), platform.arch_name());
    println!("根目录: {}", paths.root().display());
    Ok(())
}
