use anyhow::{anyhow, Result};

use crate::core::config::Config;
use crate::core::interact::{confirm, is_interactive, select};
use crate::core::links::remove_link;
use crate::core::paths::DevkitPaths;
use crate::core::shell::{rc_file_for_shell, remove_tool_injections};

/// 卸载结果：命令层据此输出提示
pub struct UninstallOutcome {
    /// 被卸载版本是否为激活版本
    pub was_active: bool,
    /// 该工具是否还有其他已安装版本
    pub has_remaining: bool,
}

pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    let paths = DevkitPaths::new()?;
    let mut config = Config::load(&paths)?;

    // 工具解析：无参时交互选择（复用 use 命令模式）
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli uninstall java"));
            }
            let tools: Vec<String> = config.installed.keys().cloned().collect();
            if tools.is_empty() {
                return Err(anyhow!("尚未安装任何工具，无需卸载"));
            }
            let idx = select("请选择要卸载的工具", &tools)?;
            tools[idx].clone()
        }
    };

    let installed = config.installed.get(&tool).cloned().unwrap_or_default();
    if installed.is_empty() {
        return Err(anyhow!(
            "{tool} 尚未安装任何版本，请先执行 cli install {tool}"
        ));
    }

    // 版本解析：指定版本需已安装；缺省时单版本直接卸、多版本交互选择
    let version = match version {
        Some(v) if installed.contains(&v) => v,
        Some(v) => {
            return Err(anyhow!(
                "{tool} {v} 未安装，可用版本: {}",
                installed.join(", ")
            ))
        }
        None => {
            if installed.len() == 1 {
                installed[0].clone()
            } else {
                let labels: Vec<String> = installed.iter().map(|v| format!("{tool} {v}")).collect();
                let idx = select("请选择要卸载的版本", &labels)?;
                installed[idx].clone()
            }
        }
    };

    // 删除确认（默认否）
    if !confirm(&format!("确认卸载 {tool} {version}？"), false)? {
        println!("已取消");
        return Ok(());
    }

    let outcome = remove_version(&paths, &mut config, &tool, &version)?;
    if outcome.was_active && outcome.has_remaining {
        println!("已卸载激活版本 {tool} {version}");
        println!("提示: 可用 `cli use {tool} <version>` 重新激活其他版本");
    } else {
        println!("已卸载 {tool} {version}");
        if !outcome.has_remaining {
            println!("该工具已无残留版本，环境已清理");
        }
    }
    Ok(())
}

/// 核心删除逻辑：删目录 → 更新 config → 删 current 链接 → 无残留时清理 shell 注入
pub fn remove_version(
    paths: &DevkitPaths,
    config: &mut Config,
    tool: &str,
    version: &str,
) -> Result<UninstallOutcome> {
    let tool_dir = paths.tool_dir(tool, version);
    if !tool_dir.exists() {
        return Err(anyhow!("{tool} {version} 未安装"));
    }
    // 1. 删版本目录（失败即中止，不动其他状态）
    std::fs::remove_dir_all(&tool_dir)?;

    // 2. 更新 config
    let was_active = config.active.get(tool).map(|s| s.as_str()) == Some(version);
    config.remove_installed(tool, version);
    if was_active {
        config.active.remove(tool);
    }
    config.save(paths)?;

    // 3. 激活版本被卸 → 删 current 链接
    if was_active {
        remove_link(&paths.current_link(tool))?;
    }

    // 4. 无残留版本 → 清理 shell 注入（失败降级警告，不阻断卸载）
    let has_remaining = config
        .installed
        .get(tool)
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !has_remaining {
        if let Ok(rc_file) = rc_file_for_shell() {
            if let Err(e) = remove_tool_injections(&rc_file, tool) {
                println!("警告: 清理 shell 配置失败: {e}");
            }
        }
    }
    Ok(UninstallOutcome {
        was_active,
        has_remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::links::set_current_link;
    use crate::core::shell::{inject_env_var, inject_path};
    use serial_test::serial;
    use tempfile::tempdir;

    /// 构造 root/java/21 + root/java/17 目录、current 链接指向 17、config active=17
    fn setup_two_versions(root: &std::path::Path) -> (DevkitPaths, Config) {
        let paths = DevkitPaths::with_root(root.to_path_buf());
        std::fs::create_dir_all(paths.tool_dir("java", "21")).unwrap();
        std::fs::create_dir_all(paths.tool_dir("java", "17")).unwrap();
        let link = paths.current_link("java");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        set_current_link(&link, std::path::Path::new("../java/17")).unwrap();
        let mut config = Config::default();
        config.add_installed("java", "21");
        config.add_installed("java", "17");
        config.set_active("java", "17");
        config.save(&paths).unwrap();
        (paths, config)
    }

    #[test]
    fn remove_non_active_version_keeps_link_and_shell() {
        let dir = tempdir().unwrap();
        let (paths, mut config) = setup_two_versions(dir.path());
        let outcome = remove_version(&paths, &mut config, "java", "21").unwrap();
        assert!(!outcome.was_active);
        assert!(outcome.has_remaining);
        assert!(!paths.tool_dir("java", "21").exists());
        assert!(paths.tool_dir("java", "17").exists());
        assert!(paths.current_link("java").symlink_metadata().is_ok()); // 链接保留
        let reloaded = Config::load(&paths).unwrap();
        assert_eq!(
            reloaded.installed.get("java").unwrap(),
            &vec!["17".to_string()]
        );
        assert_eq!(reloaded.active.get("java").unwrap(), "17");
    }

    #[test]
    fn remove_active_version_removes_link() {
        let dir = tempdir().unwrap();
        let (paths, mut config) = setup_two_versions(dir.path());
        let outcome = remove_version(&paths, &mut config, "java", "17").unwrap();
        assert!(outcome.was_active);
        assert!(outcome.has_remaining);
        assert!(paths.current_link("java").symlink_metadata().is_err()); // 链接已删
        let reloaded = Config::load(&paths).unwrap();
        assert!(!reloaded.active.contains_key("java"));
        assert_eq!(
            reloaded.installed.get("java").unwrap(),
            &vec!["21".to_string()]
        );
    }

    #[serial(env)]
    #[test]
    fn remove_last_version_cleans_injections() {
        let dir = tempdir().unwrap();
        let home = tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("SHELL", "/bin/zsh");
        let (paths, mut config) = setup_two_versions(dir.path());
        // 预置 shell 注入（PATH + JAVA_HOME 指向 current 链）
        let rc_file = home.path().join(".zshrc");
        let link = paths.current_link("java");
        inject_path(&rc_file, &link.join("bin")).unwrap();
        inject_env_var(&rc_file, "JAVA_HOME", &link.to_string_lossy()).unwrap();

        // 先卸非激活 21（不触发 shell 清理），再卸激活 17（最后版本 → 清理）
        remove_version(&paths, &mut config, "java", "21").unwrap();
        let outcome = remove_version(&paths, &mut config, "java", "17").unwrap();
        assert!(outcome.was_active);
        assert!(!outcome.has_remaining);
        assert!(link.symlink_metadata().is_err());
        let text = std::fs::read_to_string(&rc_file).unwrap();
        assert!(!text.contains("cli devkit")); // 注入整块已移除
        assert!(!text.contains("current/java"));
    }
}
