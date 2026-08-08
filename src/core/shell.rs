use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::core::paths::home_dir;

/// 插入/替换 rc 文件中由标记包裹的块。返回 true 表示新增，false 表示替换已有块。
pub fn upsert_block(rc_file: &Path, marker: &str, content: &str) -> Result<bool> {
    let start_marker = format!("# >>> cli {marker} start >>>");
    let end_marker = format!("# <<< cli {marker} end <<<");
    let block = format!("{start_marker}\n{content}\n{end_marker}\n");
    let text = if rc_file.exists() {
        std::fs::read_to_string(rc_file)?
    } else {
        String::new()
    };
    let lines: Vec<&str> = text.lines().collect();
    let start_idx = lines.iter().position(|l| l.trim() == start_marker);
    let end_idx = lines.iter().position(|l| l.trim() == end_marker);
    if let (Some(s), Some(e)) = (start_idx, end_idx) {
        if s < e {
            let mut new_text = String::new();
            for (i, line) in lines.iter().enumerate() {
                if i == s {
                    new_text.push_str(&block);
                } else if i > s && i <= e {
                    continue;
                } else {
                    new_text.push_str(line);
                    new_text.push('\n');
                }
            }
            std::fs::write(rc_file, new_text)?;
            return Ok(false);
        }
    }
    let mut new_text = text;
    if !new_text.ends_with('\n') {
        new_text.push('\n');
    }
    new_text.push_str(&block);
    std::fs::write(rc_file, new_text)?;
    Ok(true)
}

/// 读取 rc 文件中标记块的内容；无块时返回空字符串。
pub fn read_block(rc_file: &Path, marker: &str) -> Result<String> {
    if !rc_file.exists() {
        return Ok(String::new());
    }
    let text = std::fs::read_to_string(rc_file)?;
    let start_marker = format!("# >>> cli {marker} start >>>");
    let end_marker = format!("# <<< cli {marker} end <<<");
    let lines: Vec<&str> = text.lines().collect();
    let start_idx = lines.iter().position(|l| l.trim() == start_marker);
    let end_idx = lines.iter().position(|l| l.trim() == end_marker);
    if let (Some(s), Some(e)) = (start_idx, end_idx) {
        if s < e {
            return Ok(lines[s + 1..e].join("\n"));
        }
    }
    Ok(String::new())
}

/// 在 devkit 块内注入 PATH 行（POSIX 分隔符），重复调用不产生重复行。
pub fn inject_path(rc_file: &Path, dir: &Path) -> Result<()> {
    let line = format!("export PATH=\"{}:$PATH\"", dir.display());
    let current = read_block(rc_file, "devkit")?;
    let content = if current.is_empty() {
        line
    } else if current.lines().any(|l| l.trim() == line) {
        current
    } else {
        format!("{current}\n{line}")
    };
    upsert_block(rc_file, "devkit", &content)?;
    Ok(())
}

/// 在 devkit 块内 upsert 一条环境变量行（export KEY="value"），幂等
pub fn inject_env_var(rc_file: &Path, key: &str, value: &str) -> Result<()> {
    let line = format!("export {key}=\"{value}\"");
    let current = read_block(rc_file, "devkit")?;
    let content = if current.is_empty() {
        line
    } else {
        let mut kept: Vec<&str> = Vec::new();
        let mut replaced = false;
        for l in current.lines() {
            if l.starts_with(&format!("export {key}=\"")) {
                if !replaced {
                    kept.push(&line);
                    replaced = true;
                }
            } else {
                kept.push(l);
            }
        }
        if !replaced {
            kept.push(&line);
        }
        kept.join("\n")
    };
    upsert_block(rc_file, "devkit", &content)?;
    Ok(())
}

/// 根据 $SHELL 检测 rc 文件路径。Windows 暂不支持。
pub fn rc_file_for_shell() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return Err(anyhow::anyhow!("Windows 环境变量注入将在后续版本支持"));
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_default();
        let home = home_dir()?;
        if shell.ends_with("/zsh") {
            Ok(home.join(".zshrc"))
        } else if shell.ends_with("/bash") || shell.ends_with("/sh") {
            Ok(home.join(".bashrc"))
        } else {
            Err(anyhow::anyhow!(
                "无法识别的 shell: {shell}，请手动配置环境变量"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    fn upsert_block_creates_new_block() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let created = upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        assert!(created);
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains("# >>> cli devkit start >>>"));
        assert!(text.contains("export FOO=1"));
        assert!(text.contains("# <<< cli devkit end <<<"));
    }

    #[test]
    fn upsert_block_is_idempotent() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        let before = std::fs::read_to_string(&rc).unwrap();
        let created = upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        assert!(!created);
        let after = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn upsert_block_replaces_old_content() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        upsert_block(&rc, "devkit", "export FOO=2").unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains("export FOO=2"));
        assert!(!text.contains("export FOO=1"));
        assert_eq!(text.matches("# >>> cli devkit start >>>").count(), 1);
    }

    #[test]
    fn read_block_returns_content() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        assert_eq!(read_block(&rc, "devkit").unwrap(), "export FOO=1");
    }

    #[test]
    fn inject_path_is_idempotent() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let bin = dir.path().join("bin");
        inject_path(&rc, &bin).unwrap();
        inject_path(&rc, &bin).unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(
            text.matches(&format!("export PATH=\"{}:$PATH\"", bin.display()))
                .count(),
            1
        );
    }

    #[test]
    fn inject_env_var_adds_and_updates_line() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        inject_env_var(&rc, "JAVA_HOME", "/x/java").unwrap();
        inject_env_var(&rc, "JAVA_HOME", "/x/java").unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches("export JAVA_HOME=\"/x/java\"").count(), 1);
        inject_env_var(&rc, "JAVA_HOME", "/y/java").unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains("export JAVA_HOME=\"/y/java\""));
        assert!(!text.contains("/x/java"));
    }

    #[test]
    fn inject_env_var_coexists_with_path_line() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let bin = dir.path().join("bin");
        inject_path(&rc, &bin).unwrap();
        inject_env_var(&rc, "JAVA_HOME", "/x/java").unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains(&format!("export PATH=\"{}:$PATH\"", bin.display())));
        assert!(text.contains("export JAVA_HOME=\"/x/java\""));
    }

    #[serial(env)]
    #[test]
    fn rc_file_for_shell_detects_zsh() {
        let home = tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("SHELL", "/bin/zsh");
        assert_eq!(rc_file_for_shell().unwrap(), home.path().join(".zshrc"));
    }

    #[serial(env)]
    #[test]
    fn rc_file_for_shell_detects_bash() {
        let home = tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("SHELL", "/bin/bash");
        assert_eq!(rc_file_for_shell().unwrap(), home.path().join(".bashrc"));
    }

    #[serial(env)]
    #[test]
    fn rc_file_for_shell_rejects_unknown() {
        std::env::set_var("SHELL", "/bin/fish");
        let err = rc_file_for_shell().unwrap_err();
        assert!(err.to_string().contains("无法识别"));
    }
}
