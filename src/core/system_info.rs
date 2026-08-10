//! 本机系统信息检测：help「系统」行展示用。检测失败逐级回退，永不 panic。

use std::path::Path;

use crate::core::platform::Platform;

/// 麒麟操作系统判定：存在 nkvers 命令即判定为麒麟
pub fn is_kylin(nkvers_path: &Path) -> bool {
    nkvers_path.exists()
}

/// 解析 /etc/.productinfo（key=value 格式）：
/// ProductName + ProductVersion 组合基础名；ProductVersionInfo[i] 中的 `(...)` 项（SP/代号）按顺序去重拼接
pub fn parse_productinfo(content: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut marks: Vec<String> = Vec::new();
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        match k {
            "ProductName" => name = Some(v),
            "ProductVersion" => version = Some(v),
            _ if k.starts_with("ProductVersionInfo") => {
                let t = v.trim();
                if t.starts_with('(') && t.ends_with(')') && !marks.iter().any(|m| m == t) {
                    marks.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    let Some(name) = name else {
        return String::new();
    };
    let mut out = name;
    if let Some(v) = version {
        out.push(' ');
        out.push_str(&v);
    }
    for m in marks {
        out.push(' ');
        out.push_str(&m);
    }
    out
}

/// 解析 /etc/os-release：NAME + VERSION_ID（去引号）；缺 VERSION_ID 只返回 NAME
pub fn parse_os_release(content: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        match k {
            "NAME" => name = Some(v),
            "VERSION_ID" => version = Some(v),
            _ => {}
        }
    }
    match (name, version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n,
        (None, _) => String::new(),
    }
}

/// 解析 sw_vers 输出（ProductName: macOS / ProductVersion: 15.5）
pub fn parse_sw_vers(output: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    for line in output.lines() {
        if let Some(v) = line.strip_prefix("ProductName:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("ProductVersion:") {
            version = Some(v.trim().to_string());
        }
    }
    match (name, version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n,
        (None, _) => String::new(),
    }
}

/// 解析 Windows `ver` 输出（中文/英文），提取括号内版本号的前 3 段
pub fn parse_ver_output(output: &str) -> String {
    let Some(start) = output.find('[') else {
        return String::new();
    };
    let Some(end) = output.find(']') else {
        return String::new();
    };
    if end <= start + 1 {
        return String::new();
    }
    let inner = &output[start + 1..end];
    let Some(idx) = inner.find(|c: char| c.is_ascii_digit()) else {
        return String::new();
    };
    let parts: Vec<&str> = inner[idx..].split('.').take(3).collect();
    if parts.len() < 3 {
        return String::new();
    }
    format!("Windows {}", parts.join("."))
}

/// 系统名称+版本；检测失败回退基础名，永不 panic、永不为空
pub fn os_display() -> String {
    if cfg!(target_os = "macos") {
        macos_display()
    } else if cfg!(target_os = "linux") {
        linux_display()
    } else {
        windows_display()
    }
}

fn macos_display() -> String {
    match std::process::Command::new("sw_vers").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let s = parse_sw_vers(&text);
            if s.is_empty() {
                "macOS".to_string()
            } else {
                s
            }
        }
        _ => "macOS".to_string(),
    }
}

fn linux_display() -> String {
    if is_kylin(Path::new("/usr/bin/nkvers")) {
        if let Ok(text) = std::fs::read_to_string("/etc/.productinfo") {
            let s = parse_productinfo(&text);
            if !s.is_empty() {
                return s;
            }
        }
        return "Kylin Linux".to_string();
    }
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        let s = parse_os_release(&text);
        if !s.is_empty() {
            return s;
        }
    }
    "Linux".to_string()
}

fn windows_display() -> String {
    #[cfg(windows)]
    let out = std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .output();
    #[cfg(not(windows))]
    let out: Result<std::process::Output, std::io::Error> =
        Err(std::io::Error::other("not windows"));
    match out {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let s = parse_ver_output(&text);
            if s.is_empty() {
                "Windows".to_string()
            } else {
                s
            }
        }
        _ => "Windows".to_string(),
    }
}

/// help「系统」行：`系统: {os_display} ({arch})`
pub fn help_line() -> String {
    format!(
        "系统: {} ({})",
        os_display(),
        Platform::detect().arch_name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn is_kylin_detects_nkvers_file() {
        let dir = tempdir().unwrap();
        let nkvers = dir.path().join("nkvers");
        assert!(!is_kylin(&nkvers));
        std::fs::write(&nkvers, b"#!/bin/sh\n").unwrap();
        assert!(is_kylin(&nkvers));
    }

    #[test]
    fn parse_productinfo_full_with_sp_and_codename() {
        let content = "\
ProductName=Kylin Linux Advanced Server
ProductVersion=V10
ProductType=Server
ProductVersionInfo[0]=Kylin Linux Advanced Server V10
ProductVersionInfo[1]=(SP1)
ProductVersionInfo[2]=(Halberd)
ProductVersionInfo[3]=Kylin Linux Advanced Server V10
ProductVersionInfo[4]=(SP1)
ProductVersionInfo[5]=(Halberd)";
        assert_eq!(
            parse_productinfo(content),
            "Kylin Linux Advanced Server V10 (SP1) (Halberd)"
        );
    }

    #[test]
    fn parse_productinfo_missing_version_keeps_name_and_marks() {
        let content = "ProductName=Kylin Linux Desktop\nProductVersionInfo[1]=(SP1)";
        assert_eq!(parse_productinfo(content), "Kylin Linux Desktop (SP1)");
    }

    #[test]
    fn parse_productinfo_empty_returns_empty() {
        assert_eq!(parse_productinfo(""), "");
        assert_eq!(parse_productinfo("ProductType=Server\n"), "");
    }

    #[test]
    fn parse_os_release_quoted() {
        let content = "NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\n";
        assert_eq!(parse_os_release(content), "Ubuntu 22.04");
    }

    #[test]
    fn parse_os_release_unquoted_and_missing_version() {
        assert_eq!(parse_os_release("NAME=CentOS Linux\n"), "CentOS Linux");
        assert_eq!(parse_os_release("VERSION_ID=7\n"), "");
    }

    #[test]
    fn parse_sw_vers_standard() {
        let output = "ProductName:\t\tmacOS\nProductVersion:\t\t15.5\nBuildVersion:\t\t24D60\n";
        assert_eq!(parse_sw_vers(output), "macOS 15.5");
    }

    #[test]
    fn parse_ver_output_english() {
        let output = "Microsoft Windows [Version 10.0.22631.4460]\n";
        assert_eq!(parse_ver_output(output), "Windows 10.0.22631");
    }

    #[test]
    fn parse_ver_output_chinese() {
        let output = "Microsoft Windows [版本 10.0.22631]\n";
        assert_eq!(parse_ver_output(output), "Windows 10.0.22631");
    }

    #[test]
    fn parse_ver_output_unmatched_returns_empty() {
        assert_eq!(parse_ver_output("not windows output"), "");
    }

    #[test]
    fn os_display_never_empty() {
        assert!(!os_display().is_empty());
    }

    #[test]
    fn help_line_contains_arch() {
        let line = help_line();
        assert!(line.starts_with("系统: "));
        assert!(
            line.contains("x86_64") || line.contains("aarch64"),
            "help_line 必须包含架构: {line}"
        );
    }
}
