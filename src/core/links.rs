use std::path::Path;

use anyhow::{anyhow, Result};

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(|e| anyhow!("创建符号链接失败: {e}"))
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
            .map_err(|e| anyhow!("创建符号链接失败（可能需要管理员权限）: {e}"))
    } else {
        std::os::windows::fs::symlink_file(target, link)
            .map_err(|e| anyhow!("创建符号链接失败（可能需要管理员权限）: {e}"))
    }
}

/// 原子设置 current 符号链接：先建临时链接再 rename 覆盖；目标相同则跳过
pub fn set_current_link(link: &Path, target: &Path) -> Result<()> {
    if let Ok(cur) = std::fs::read_link(link) {
        if cur == target {
            return Ok(()); // 幂等
        }
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = link.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    create_symlink(target, &tmp)?;
    if link.symlink_metadata().is_ok() {
        #[cfg(windows)]
        std::fs::remove_file(link)?;
    }
    std::fs::rename(&tmp, link)?;
    Ok(())
}

/// 移除 current 符号链接；不存在时返回 false，移除成功返回 true
pub fn remove_link(link: &Path) -> Result<bool> {
    #[cfg(not(windows))]
    {
        if link.symlink_metadata().is_err() {
            return Ok(false);
        }
        std::fs::remove_file(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
    }
    #[cfg(windows)]
    {
        let Ok(meta) = link.symlink_metadata() else {
            return Ok(false);
        };
        if meta.file_type().is_dir() {
            std::fs::remove_dir(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
        } else {
            std::fs::remove_file(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn set_current_link_creates_and_updates() {
        let dir = tempdir().unwrap();
        let t1 = dir.path().join("java21");
        let t2 = dir.path().join("java8");
        std::fs::create_dir_all(&t1).unwrap();
        std::fs::create_dir_all(&t2).unwrap();
        let link = dir.path().join("current").join("java");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();

        set_current_link(&link, &t1).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), t1);

        // 切换到新目标
        set_current_link(&link, &t2).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), t2);

        // 幂等：同目标不报错不变化
        set_current_link(&link, &t2).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), t2);
    }

    #[test]
    fn remove_link_removes_existing_link() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("java21");
        std::fs::create_dir_all(&target).unwrap();
        let link = dir.path().join("current").join("java");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        set_current_link(&link, &target).unwrap();
        assert!(remove_link(&link).unwrap());
        assert!(link.symlink_metadata().is_err());
    }

    #[test]
    fn remove_link_noop_when_absent() {
        let dir = tempdir().unwrap();
        let link = dir.path().join("current").join("java");
        assert!(!remove_link(&link).unwrap());
    }
}
