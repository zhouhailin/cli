/// CLI_DEBUG=true 时输出调试日志（输出到 stderr，避免污染 stdout 管道）
pub fn is_debug_enabled() -> bool {
    std::env::var("CLI_DEBUG").map(|v| v == "true").unwrap_or(false)
}

/// 调试日志宏：仅当 CLI_DEBUG=true 时输出 `[debug] 消息` 到 stderr
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if $crate::core::debug::is_debug_enabled() {
            eprintln!("[debug] {}", format!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[serial(env)]
    #[test]
    fn debug_disabled_by_default() {
        std::env::remove_var("CLI_DEBUG");
        assert!(!is_debug_enabled());
    }

    #[serial(env)]
    #[test]
    fn debug_enabled_with_true() {
        std::env::set_var("CLI_DEBUG", "true");
        assert!(is_debug_enabled());
    }

    #[serial(env)]
    #[test]
    fn debug_ignores_other_values() {
        std::env::set_var("CLI_DEBUG", "1");
        assert!(!is_debug_enabled());
        std::env::set_var("CLI_DEBUG", "TRUE");
        assert!(!is_debug_enabled());
    }
}
