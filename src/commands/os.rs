use anyhow::{anyhow, Result};

use crate::core::mirror;
use crate::OsCommand;

pub fn run(cmd: OsCommand) -> Result<()> {
    match cmd {
        OsCommand::List => run_list(),
        OsCommand::Info { name } => run_info(&name),
        OsCommand::Download {
            name,
            version,
            output_dir,
        } => run_download(&name, version.as_deref(), &output_dir),
    }
}

pub fn run_list() -> Result<()> {
    let names = mirror::fetch_all_names()?;
    if names.is_empty() {
        println!("暂无可用系统镜像");
        return Ok(());
    }
    for n in names {
        println!("{n}");
    }
    Ok(())
}

pub fn run_info(name: &str) -> Result<()> {
    let images = mirror::fetch_images(name)?;
    if images.is_empty() {
        println!("系统 {name} 暂无可用镜像");
        return Ok(());
    }
    println!("{name} 共 {} 个镜像:", images.len());
    for (i, img) in images.iter().enumerate() {
        println!(
            " {}  {:<28} {:<10} {}  {}",
            i + 1,
            img.version,
            mirror::format_size(img.size),
            img.last_update_time.as_deref().unwrap_or("-"),
            img.download_url
        );
    }
    Ok(())
}

pub fn run_download(
    name: &str,
    version: Option<&str>,
    output_dir: &str,
) -> Result<()> {
    // Task 5 实现
    Err(anyhow!("download 尚未实现"))
}
