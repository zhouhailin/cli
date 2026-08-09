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

/// 构建安装命令：curl 脚本 | sh 非交互安装；--no-modify-path 保证 rc 由 cli 统一注入。
/// 测试钩子指向 http:// 时放宽 --proto 限制（--proto '=https' 会拒绝 http）
pub fn install_command(source: RustSource) -> String {
    let url = source.script_url();
    let proto = if url.starts_with("http://") {
        ""
    } else {
        "--proto '=https' --tlsv1.2 "
    };
    format!("curl {proto}-sSf {url} | sh -s -- -y --no-modify-path")
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
        vars.push((
            "RUSTUP_UPDATE_ROOT".to_string(),
            ALIYUN_UPDATE_ROOT.to_string(),
        ));
        vars.push((
            "RUSTUP_DIST_SERVER".to_string(),
            ALIYUN_DIST_SERVER.to_string(),
        ));
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

/// 已安装检测：<root>/rustup/bin/rustup 存在（rustup 二进制就位）才算安装完成。
/// 仅目录存在（上次安装失败残留）不算已安装，避免残留目录阻塞重试。
pub fn is_installed(root: &Path) -> bool {
    rustup_home_dir(root).join("bin").join("rustup").exists()
}

/// 未完成安装残留检测：<root>/rustup 目录存在但 rustup 二进制未就位。
/// 返回残留目录路径，供安装流程警告与清理指引使用。
pub fn install_residual(root: &Path) -> Option<PathBuf> {
    let home = rustup_home_dir(root);
    if home.exists() && !home.join("bin").join("rustup").exists() {
        Some(home)
    } else {
        None
    }
}

/// 清理残留的命令提示（rustup 目录 + cargo 目录）
pub fn clean_residual_hint(root: &Path) -> String {
    format!(
        "rm -rf {} {}",
        rustup_home_dir(root).display(),
        cargo_home_dir(root).display()
    )
}

/// 注册 rust 到 config.json 供 `cli list` 显示（仅展示，不参与 use/uninstall）
pub fn register_in_config(root: &Path) -> Result<()> {
    use crate::core::config::Config;
    use crate::core::paths::DevkitPaths;

    let paths = DevkitPaths::with_root(root.to_path_buf());
    let mut config = Config::load(&paths)?;
    config.add_installed("rust", "rustup");
    config.save(&paths)
}

/// 安装 Rust（rustup）：选择源 → 执行脚本 → 持久化环境变量与 PATH
pub fn install(_hint: Option<&str>) -> Result<()> {
    #[cfg(windows)]
    {
        return Err(anyhow::anyhow!(
            "Windows 暂不支持自动安装 Rust，请手动运行 rustup-init.exe（https://rustup.rs）"
        ));
    }
    #[cfg(not(windows))]
    {
        use crate::core::shell::{
            inject_env_var, inject_path, print_activation_hint, rc_file_for_shell,
        };

        let paths = crate::core::paths::DevkitPaths::new()?;
        let root = paths.root();
        if is_installed(root) {
            return Err(anyhow::anyhow!(
                "Rust (rustup) 已安装于 {}，如需重装请先手动删除该目录与 rc 中相关注入",
                rustup_home_dir(root).display()
            ));
        }
        // 存在未完成残留（上次安装失败/中断）：警告并继续，rustup 可基于已有 settings 续装；
        // 若续装异常可先清理残留重试
        if let Some(residual) = install_residual(root) {
            println!(
                "警告: 检测到上次未完成的安装残留（{}），将尝试继续安装；\n      若安装异常，可先清理后重试：{}",
                residual.display(),
                clean_residual_hint(root)
            );
        }
        let source = RustSource::choose()?;
        println!("开始安装 Rust（{}）...", source.label());
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(install_command(source))
            .envs(install_env_vars(source, root))
            .status()?;
        if !status.success() {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "信号终止".to_string());
            return Err(anyhow::anyhow!(
                "Rust 安装失败，退出码 {code}。常见原因：内存不足（rustup 解压 rustc 需要较多内存，\n可通过 free -h 查看并用 swap 缓解）、网络问题或残留损坏。\n可清理后重试：{}",
                clean_residual_hint(root)
            ));
        }
        // 持久化环境变量与 PATH（失败仅警告，不阻断已完成的安装）
        let rc = rc_file_for_shell()?;
        for (key, value) in install_env_vars(source, root) {
            if let Err(e) = inject_env_var(&rc, &key, &value) {
                eprintln!(
                    "警告: 环境变量 {key} 写入 {} 失败: {e}，请手动配置",
                    rc.display()
                );
            }
        }
        if let Err(e) = inject_path(&rc, &cargo_home_dir(root).join("bin")) {
            eprintln!("警告: PATH 写入 {} 失败: {e}，请手动配置", rc.display());
        }
        // 注册到 config.json 供 cli list 显示（失败仅警告，不阻断已完成的安装）
        if let Err(e) = register_in_config(root) {
            eprintln!("警告: 写入 config.json 失败: {e}，cli list 将不显示 rust");
        }
        println!("Rust (rustup) 安装完成（{}）", source.label());
        print_activation_hint()?;
        Ok(())
    }
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
        assert!(install_command(RustSource::Official).contains("http://127.0.0.1:9/rustup-init.sh"));
        std::env::remove_var("DEVKIT_RUSTUP_SCRIPT");
    }

    #[test]
    fn aliyun_env_vars_returns_two_entries() {
        assert_eq!(
            aliyun_env_vars(),
            vec![
                (
                    "RUSTUP_UPDATE_ROOT",
                    "https://mirrors.aliyun.com/rustup/rustup"
                ),
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
    fn is_installed_requires_rustup_binary() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_installed(dir.path()));
        // 仅目录存在（残留/未完成安装）不算已安装
        std::fs::create_dir_all(dir.path().join("rustup")).unwrap();
        assert!(!is_installed(dir.path()));
        // rustup 二进制就位才算安装完成
        std::fs::create_dir_all(dir.path().join("rustup/bin")).unwrap();
        std::fs::write(dir.path().join("rustup/bin/rustup"), "").unwrap();
        assert!(is_installed(dir.path()));
    }

    #[test]
    fn install_residual_detects_incomplete_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(install_residual(dir.path()).is_none());
        // 有目录但 rustup 二进制未就位 → 残留
        std::fs::create_dir_all(dir.path().join("rustup")).unwrap();
        assert_eq!(
            install_residual(dir.path()),
            Some(rustup_home_dir(dir.path()))
        );
        // 二进制就位后不再是残留
        std::fs::create_dir_all(dir.path().join("rustup/bin")).unwrap();
        std::fs::write(dir.path().join("rustup/bin/rustup"), "").unwrap();
        assert!(install_residual(dir.path()).is_none());
    }

    #[test]
    fn register_in_config_records_rust() {
        use crate::core::config::Config;
        use crate::core::paths::DevkitPaths;

        let dir = tempfile::tempdir().unwrap();
        register_in_config(dir.path()).unwrap();
        let paths = DevkitPaths::with_root(dir.path().to_path_buf());
        let config = Config::load(&paths).unwrap();
        assert_eq!(
            config.installed.get("rust"),
            Some(&vec!["rustup".to_string()])
        );
        // 幂等：重复注册不产生重复条目
        register_in_config(dir.path()).unwrap();
        let config = Config::load(&paths).unwrap();
        assert_eq!(
            config.installed.get("rust"),
            Some(&vec!["rustup".to_string()])
        );
    }

    #[test]
    fn source_labels_are_chinese() {
        assert_eq!(RustSource::Official.label(), "官方源");
        assert_eq!(RustSource::Aliyun.label(), "阿里源");
    }
}
