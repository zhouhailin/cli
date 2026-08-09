use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;
use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};

/// 从 Apache archive 目录页 HTML 提取版本号（纯数字点分），降序
pub fn parse_maven_versions(html: &str) -> Result<Vec<String>> {
    crate::core::versions::parse_version_dirs(html)
}

/// 版本号比较（点分段数字），复用 core::versions
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    crate::core::versions::compare(a, b)
}

/// maven 下载 URL
pub fn resolve_url(version: &str) -> String {
    format!(
        "https://archive.apache.org/dist/maven/maven-3/{version}/binaries/apache-maven-{version}-bin.tar.gz"
    )
}

/// 生成含阿里云镜像的 settings.xml
pub fn generate_settings_xml(local_repo: &str, mirror_url: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<settings>
  <localRepository>{local_repo}</localRepository>
  <mirrors>
    <mirror>
      <id>aliyunmaven</id>
      <name>阿里云公共仓库</name>
      <mirrorOf>central</mirrorOf>
      <url>{mirror_url}</url>
    </mirror>
  </mirrors>
</settings>
"#
    )
}

/// 写入镜像 settings.xml；目标已存在（发行包自带/用户改过）时先备份为 settings.xml.backup，
/// 返回是否执行了备份
pub fn write_settings_with_backup(tool_dir: &Path, settings: &str) -> Result<bool> {
    let conf = tool_dir.join("conf");
    std::fs::create_dir_all(&conf)?;
    let settings_path = conf.join("settings.xml");
    let backup_path = conf.join("settings.xml.backup");
    let backed_up = if settings_path.exists() {
        std::fs::copy(&settings_path, &backup_path)
            .map_err(|e| anyhow!("备份 settings.xml 失败: {e}"))?;
        true
    } else {
        false
    };
    std::fs::write(&settings_path, settings)?;
    Ok(backed_up)
}

pub fn install(version_hint: Option<&str>) -> Result<()> {
    let body = http_get_string("https://archive.apache.org/dist/maven/maven-3/")?;
    let list = parse_maven_versions(&body)?;
    let version = if let Some(hint) = version_hint {
        if !list.contains(&hint.to_string()) {
            return Err(anyhow!("版本 {hint} 不可用，请从列表中选择"));
        }
        hint.to_string()
    } else {
        let labels: Vec<String> = list.iter().map(|v| format!("Maven {v}")).collect();
        let idx = select("请选择 Maven 版本", &labels)?;
        list[idx].clone()
    };
    let url = resolve_url(&version);
    println!("准备安装 Maven {version}...");
    println!("下载地址: {url}");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let mut ctx = InstallContext::load()?;
    install_archive(&url, None, "maven", &version, &mut ctx, false)?;
    // 写入阿里云镜像 settings.xml（原文件先备份为 settings.xml.backup）
    let tool_dir = ctx.paths.tool_dir("maven", &version);
    let local_repo = ctx.paths.root().join("maven").join("repository");
    let settings = generate_settings_xml(
        &local_repo.to_string_lossy(),
        "https://maven.aliyun.com/repository/public",
    );
    if write_settings_with_backup(&tool_dir, &settings)? {
        println!("已备份原始 settings.xml 为 settings.xml.backup");
    }
    let rc_file = crate::core::shell::rc_file_for_shell()?;
    crate::core::shell::inject_path(&rc_file, &ctx.paths.current_link("maven").join("bin"))?;
    crate::core::shell::print_activation_hint()?;
    println!("Maven {version} 安装完成（已配置阿里云镜像与本地仓库）");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maven_versions_extracts_and_sorts_desc() {
        let html = r#"<html><body>
          <a href="3.9.8/">3.9.8/</a>
          <a href="3.9.10/">3.9.10/</a>
          <a href="3.9.9/">3.9.9/</a>
          <a href="README.html">README</a>
        </body></html>"#;
        let list = parse_maven_versions(html).unwrap();
        assert_eq!(list, vec!["3.9.10", "3.9.9", "3.9.8"]);
    }

    #[test]
    fn compare_versions_padded_segments() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("3.9.10", "3.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("3.9.9", "3.9.9"), Ordering::Equal);
        assert_eq!(compare_versions("3.8", "3.9.1"), Ordering::Less);
    }

    #[test]
    fn resolve_url_format() {
        assert_eq!(
            resolve_url("3.9.9"),
            "https://archive.apache.org/dist/maven/maven-3/3.9.9/binaries/apache-maven-3.9.9-bin.tar.gz"
        );
    }

    #[test]
    fn generate_settings_xml_contains_aliyun_mirror() {
        let xml = generate_settings_xml("/root/repo", "https://maven.aliyun.com/repository/public");
        assert!(xml.contains("aliyunmaven"));
        assert!(xml.contains("https://maven.aliyun.com/repository/public"));
        assert!(xml.contains("<localRepository>/root/repo</localRepository>"));
    }

    #[test]
    fn write_settings_with_backup_backs_up_existing() {
        let dir = tempfile::tempdir().unwrap();
        let tool_dir = dir.path().join("apache-maven-3.9.9");
        let conf = tool_dir.join("conf");
        std::fs::create_dir_all(&conf).unwrap();
        std::fs::write(
            conf.join("settings.xml"),
            "<settings><!-- default --></settings>",
        )
        .unwrap();
        let backed_up =
            write_settings_with_backup(&tool_dir, "<settings><mirror/></settings>").unwrap();
        assert!(backed_up);
        assert_eq!(
            std::fs::read_to_string(conf.join("settings.xml")).unwrap(),
            "<settings><mirror/></settings>"
        );
        assert_eq!(
            std::fs::read_to_string(conf.join("settings.xml.backup")).unwrap(),
            "<settings><!-- default --></settings>"
        );
    }

    #[test]
    fn write_settings_with_backup_skips_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let tool_dir = dir.path().join("apache-maven-3.9.9");
        let backed_up = write_settings_with_backup(&tool_dir, "<settings/>").unwrap();
        assert!(!backed_up);
        assert_eq!(
            std::fs::read_to_string(tool_dir.join("conf").join("settings.xml")).unwrap(),
            "<settings/>"
        );
        assert!(!tool_dir.join("conf").join("settings.xml.backup").exists());
    }
}
