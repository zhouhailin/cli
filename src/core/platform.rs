use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn detect() -> Platform {
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            panic!("不支持的操作系统");
        };
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else {
            panic!("不支持的架构");
        };
        Platform { os, arch }
    }

    pub fn os_name(&self) -> &'static str {
        match self.os {
            Os::MacOs => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }

    pub fn arch_name(&self) -> &'static str {
        match self.arch {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.os_name(), self.arch_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_current_platform() {
        let p = Platform::detect();
        #[cfg(target_os = "macos")]
        assert_eq!(p.os, Os::MacOs);
        #[cfg(target_os = "linux")]
        assert_eq!(p.os, Os::Linux);
        #[cfg(target_os = "windows")]
        assert_eq!(p.os, Os::Windows);
        #[cfg(target_arch = "x86_64")]
        assert_eq!(p.arch, Arch::X86_64);
        #[cfg(target_arch = "aarch64")]
        assert_eq!(p.arch, Arch::Aarch64);
    }

    #[test]
    fn os_name_maps_correctly() {
        assert_eq!(Platform { os: Os::MacOs, arch: Arch::Aarch64 }.os_name(), "macos");
        assert_eq!(Platform { os: Os::Linux, arch: Arch::Aarch64 }.os_name(), "linux");
        assert_eq!(Platform { os: Os::Windows, arch: Arch::Aarch64 }.os_name(), "windows");
    }

    #[test]
    fn arch_name_maps_correctly() {
        assert_eq!(Platform { os: Os::Linux, arch: Arch::X86_64 }.arch_name(), "x86_64");
        assert_eq!(Platform { os: Os::Linux, arch: Arch::Aarch64 }.arch_name(), "aarch64");
    }

    #[test]
    fn display_shows_os_and_arch() {
        let p = Platform { os: Os::MacOs, arch: Arch::Aarch64 };
        assert_eq!(p.to_string(), "macos (aarch64)");
    }
}
