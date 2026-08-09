use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::download;
use crate::core::interact::{confirm, is_interactive, select};
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

pub fn run_download(name: &str, version: Option<&str>, output_dir: &str) -> Result<()> {
    let images = mirror::fetch_images(name)?;
    if images.is_empty() {
        return Err(anyhow!("系统 {name} 暂无可用镜像"));
    }
    println!("{name} 共 {} 个镜像:", images.len());
    for (i, img) in images.iter().enumerate() {
        println!(
            " {}  {:<28} {:<10} {}",
            i + 1,
            img.version,
            mirror::format_size(img.size),
            img.download_url
        );
    }
    let selected = match version {
        Some(v) => mirror::find_image_by_version(&images, v).ok_or_else(|| {
            let avail: Vec<&str> = images.iter().map(|i| i.version.as_str()).collect();
            anyhow!("未找到版本 {v}，可用版本: {}", avail.join("、"))
        })?,
        None => {
            if !is_interactive() {
                return Err(anyhow!(
                    "非终端环境请通过 --version 指定镜像版本，例如: cli os download {name} --version <版本>"
                ));
            }
            let labels: Vec<String> = images
                .iter()
                .map(|i| format!("{}  ({})", i.version, mirror::format_size(i.size)))
                .collect();
            let idx = select(&format!("请选择要下载的 {name} 镜像"), &labels)?;
            &images[idx]
        }
    };
    let dir = Path::new(output_dir);
    std::fs::create_dir_all(dir)?;
    let file_name = mirror::file_name_from_url(&selected.download_url)?;
    let mut dest = dir.join(&file_name);
    if dest.exists() {
        if !is_interactive() {
            println!("目标文件已存在，跳过: {}", dest.display());
            return Ok(());
        }
        let idx = select(
            "文件已存在，请选择处理方式",
            &["覆盖下载", "跳过", "重命名"],
        )?;
        match idx {
            0 => {}
            1 => {
                println!("已跳过");
                return Ok(());
            }
            _ => {
                let mut n = 1;
                while mirror::renamed_path(dir, &file_name, n).exists() {
                    n += 1;
                }
                dest = mirror::renamed_path(dir, &file_name, n);
            }
        }
    }
    println!("准备下载 {name} {} → {}", selected.version, dest.display());
    println!(
        "大小: {} | 链接: {}",
        mirror::format_size(selected.size),
        selected.download_url
    );
    if is_interactive() && !confirm("确认开始下载？", true)? {
        println!("已取消");
        return Ok(());
    }
    download::download(
        &selected.download_url,
        &dest,
        None,
        &format!("{name} {}", selected.version),
    )?;
    mirror::verify_image_md5(&dest, selected)?;
    println!("下载完成: {}", dest.display());
    Ok(())
}
