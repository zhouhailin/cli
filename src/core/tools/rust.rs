//! Rust (rustup) 安装支持：阿里源/官方源双源选择，安装到 devkit 目录

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::core::interact::{is_interactive, select};

/// 官方源安装脚本 URL
pub const OFFICIAL_SCRIPT_URL: &str = "https://sh.rustup.rs";
/// 阿里源安装脚本 URL
pub const ALIYUN_SCRIPT_URL: &str = "https://mirrors.aliyun.com/repo/rust/rustup-init.sh";
/// 阿里源发行版镜像根（RUSTUP_DIST_SERVER）
pub const ALIYUN_DIST_SERVER: &str = "https://mirrors.aliyun.com/rustup";
/// 阿里源更新元数据（RUSTUP_UPDATE_ROOT）
pub const ALIYUN_UPDATE_ROOT: &str = "https://mirrors.aliyun.com/rustup/rustup";

/// Rust 下载源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustSource {
    Official,
    Aliyun,
}

impl RustSource {
    /// 源的中文标签
    pub fn label(self) -> &'static str {
        match self {
            RustSource::Official => "官方源",
            RustSource::Aliyun => "阿里源",
        }
    }

    /// 安装脚本 URL；DEVKIT_RUSTUP_SCRIPT 非空时覆盖（测试钩子）
    pub fn script_url(self) -> String {
        if let Ok(url) = std::env::var("DEVKIT_RUSTUP_SCRIPT") {
            if !url.is_empty() {
                return url;
            }
        }
        match self {
            RustSource::Official => OFFICIAL_SCRIPT_URL.to_string(),
            RustSource::Aliyun => ALIYUN_SCRIPT_URL.to_string(),
        }
    }

    /// 交互选择下载源（阿里源在前）；非 TTY 默认官方源
    pub fn choose() -> Result<RustSource> {
        if !is_interactive() {
            println!("提示: 非交互模式默认使用官方源，可通过交互模式选择阿里源");
            return Ok(RustSource::Official);
        }
        let labels = ["阿里源（国内加速）", "官方源"];
        let idx = select("请选择 Rust 下载源", &labels)?;
        Ok(if idx == 0 {
            RustSource::Aliyun
        } else {
            RustSource::Official
        })
    }
}

/// 构建安装命令：curl 脚本 | sh 非交互安装；--no-modify-path 保证 rc 由 cli 统一注入
pub fn install_command(source: RustSource) -> String {
    format!(
        "curl --proto '=https' --tlsv1.2 -sSf {} | sh -s -- -y --no-modify-path",
        source.script_url()
    )
}

/// rustup 主目录：<root>/rustup
pub fn rustup_home_dir(root: &Path) -> PathBuf {
    root.join("rustup")
}

/// cargo 主目录：<root>/cargo
pub fn cargo_home_dir(root: &Path) -> PathBuf {
    root.join("cargo")
}

/// 安装进程注入的环境变量：RUSTUP_HOME/CARGO_HOME + 阿里源镜像变量
pub fn install_env_vars(source: RustSource, root: &Path) -> Vec<(String, String)> {
    let mut vars = vec![
        (
            "RUSTUP_HOME".to_string(),
            rustup_home_dir(root).display().to_string(),
        ),
        (
            "CARGO_HOME".to_string(),
            cargo_home_dir(root).display().to_string(),
        ),
    ];
    if source == RustSource::Aliyun {
        vars.push(("RUSTUP_UPDATE_ROOT".to_string(), ALIYUN_UPDATE_ROOT.to_string()));
        vars.push(("RUSTUP_DIST_SERVER".to_string(), ALIYUN_DIST_SERVER.to_string()));
    }
    vars
}

/// 阿里源镜像环境变量（安装成功后的 rc 持久化）
pub fn aliyun_env_vars() -> Vec<(&'static str, &'static str)> {
    vec![
        ("RUSTUP_UPDATE_ROOT", ALIYUN_UPDATE_ROOT),
        ("RUSTUP_DIST_SERVER", ALIYUN_DIST_SERVER),
    ]
}

/// 已安装检测：<root>/rustup 目录存在即视为已安装
pub fn is_installed(root: &Path) -> bool {
    rustup_home_dir(root).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[serial(env)]
    #[test]
    fn install_command_official_source() {
        assert_eq!(
            install_command(RustSource::Official),
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path"
        );
    }

    #[serial(env)]
    #[test]
    fn install_command_aliyun_source() {
        assert_eq!(
            install_command(RustSource::Aliyun),
            "curl --proto '=https' --tlsv1.2 -sSf https://mirrors.aliyun.com/repo/rust/rustup-init.sh | sh -s -- -y --no-modify-path"
        );
    }

    #[serial(env)]
    #[test]
    fn install_command_uses_script_override() {
        std::env::set_var("DEVKIT_RUSTUP_SCRIPT", "http://127.0.0.1:9/rustup-init.sh");
        assert!(
            install_command(RustSource::Official)
                .contains("http://127.0.0.1:9/rustup-init.sh")
        );
        std::env::remove_var("DEVKIT_RUSTUP_SCRIPT");
    }

    #[test]
    fn aliyun_env_vars_returns_two_entries() {
        assert_eq!(
            aliyun_env_vars(),
            vec![
                ("RUSTUP_UPDATE_ROOT", "https://mirrors.aliyun.com/rustup/rustup"),
                ("RUSTUP_DIST_SERVER", "https://mirrors.aliyun.com/rustup"),
            ]
        );
    }

    #[test]
    fn common_env_vars_include_home_and_cargo() {
        let root = Path::new("/tmp/devkit");
        let vars = install_env_vars(RustSource::Official, root);
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&("RUSTUP_HOME".to_string(), "/tmp/devkit/rustup".to_string())));
        assert!(vars.contains(&("CARGO_HOME".to_string(), "/tmp/devkit/cargo".to_string())));
    }

    #[test]
    fn install_env_vars_aliyun_adds_mirror_vars() {
        let root = Path::new("/tmp/devkit");
        let vars = install_env_vars(RustSource::Aliyun, root);
        assert_eq!(vars.len(), 4);
        assert!(vars.iter().any(|(k, v)| {
            k == "RUSTUP_UPDATE_ROOT" && v == "https://mirrors.aliyun.com/rustup/rustup"
        }));
        assert!(vars.iter().any(|(k, v)| {
            k == "RUSTUP_DIST_SERVER" && v == "https://mirrors.aliyun.com/rustup"
        }));
    }

    #[test]
    fn rustup_home_dir_from_root() {
        assert_eq!(
            rustup_home_dir(Path::new("/tmp/devkit")),
            PathBuf::from("/tmp/devkit/rustup")
        );
    }

    #[test]
    fn is_installed_detects_rustup_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_installed(dir.path()));
        std::fs::create_dir_all(dir.path().join("rustup")).unwrap();
        assert!(is_installed(dir.path()));
    }

    #[test]
    fn source_labels_are_chinese() {
        assert_eq!(RustSource::Official.label(), "官方源");
        assert_eq!(RustSource::Aliyun.label(), "阿里源");
    }
}
