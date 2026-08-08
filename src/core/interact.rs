use anyhow::{anyhow, Result};

/// stdin 与 stderr 均为终端时才视为交互式（dialoguer 基于 stderr 渲染）
pub fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// 单选列表，返回选中项下标；非 TTY 或取消时返回中文错误
pub fn select<T: ToString>(title: &str, items: &[T]) -> Result<usize> {
    if items.is_empty() {
        return Err(anyhow!("选项列表为空"));
    }
    if !is_interactive() {
        return Err(anyhow!(
            "当前环境不支持交互选择（非终端），请使用非交互参数"
        ));
    }
    let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(title)
        .items(items)
        .default(0)
        .interact()
        .map_err(|e| anyhow!("交互选择失败: {e}"))?;
    Ok(selection)
}

/// 确认框，返回 true/false；非 TTY 时返回错误
pub fn confirm(title: &str, default: bool) -> Result<bool> {
    if !is_interactive() {
        return Err(anyhow!(
            "当前环境不支持交互确认（非终端），请使用非交互参数"
        ));
    }
    let result = dialoguer::Confirm::new()
        .with_prompt(title)
        .default(default)
        .interact()
        .map_err(|e| anyhow!("交互确认失败: {e}"))?;
    Ok(result)
}

/// 文本输入；非 TTY 时返回错误
pub fn input(title: &str, default: Option<&str>) -> Result<String> {
    if !is_interactive() {
        return Err(anyhow!(
            "当前环境不支持交互输入（非终端），请使用非交互参数"
        ));
    }
    let mut builder = dialoguer::Input::<String>::new().with_prompt(title);
    if let Some(d) = default {
        builder = builder.default(d.to_string());
    }
    let value = builder
        .interact()
        .map_err(|e| anyhow!("交互输入失败: {e}"))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_rejects_empty_list() {
        let items: Vec<&str> = vec![];
        let err = select("标题", &items).unwrap_err();
        assert!(err.to_string().contains("选项列表为空"));
    }

    #[test]
    fn select_rejects_non_tty() {
        // 测试进程 stdin 非 TTY（CI 环境）；若本地为 TTY 则此测试跳过
        if is_interactive() {
            return;
        }
        let items = vec!["a", "b"];
        let err = select("标题", &items).unwrap_err();
        assert!(err.to_string().contains("非终端"));
    }

    #[test]
    fn confirm_rejects_non_tty() {
        if is_interactive() {
            return;
        }
        let err = confirm("确认?", true).unwrap_err();
        assert!(err.to_string().contains("非终端"));
    }
}
