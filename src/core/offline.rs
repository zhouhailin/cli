//! 离线模式检测：CLI_OFFLINE / DEVKIT_OFFLINE 任一非空且非 0/false 即启用

/// 是否处于离线模式（仅使用本地缓存，不访问网络）
pub fn is_offline() -> bool {
    ["CLI_OFFLINE", "DEVKIT_OFFLINE"].iter().any(|key| {
        std::env::var(key)
            .map(|v| {
                let t = v.trim().to_lowercase();
                !t.is_empty() && t != "0" && t != "false"
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn clear_vars() {
        std::env::remove_var("CLI_OFFLINE");
        std::env::remove_var("DEVKIT_OFFLINE");
    }

    #[serial(env)]
    #[test]
    fn offline_when_cli_offline_true() {
        clear_vars();
        std::env::set_var("CLI_OFFLINE", "true");
        assert!(is_offline());
    }

    #[serial(env)]
    #[test]
    fn offline_when_devkit_offline_one() {
        clear_vars();
        std::env::set_var("DEVKIT_OFFLINE", "1");
        assert!(is_offline());
    }

    #[serial(env)]
    #[test]
    fn online_when_vars_unset() {
        clear_vars();
        assert!(!is_offline());
    }

    #[serial(env)]
    #[test]
    fn online_when_var_is_false_or_zero() {
        clear_vars();
        std::env::set_var("CLI_OFFLINE", "false");
        assert!(!is_offline());
        std::env::set_var("CLI_OFFLINE", "0");
        assert!(!is_offline());
        std::env::set_var("CLI_OFFLINE", "FALSE");
        assert!(!is_offline());
    }

    #[serial(env)]
    #[test]
    fn offline_when_var_is_uppercase_true() {
        clear_vars();
        std::env::set_var("DEVKIT_OFFLINE", "TRUE");
        assert!(is_offline());
    }
}
