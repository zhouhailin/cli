pub mod commands;
pub mod core;

use clap::{Parser, Subcommand};

use crate::core::versions::parse_tag;

/// 当前版本：发布构建用 CLI_VERSION（tag），本地开发回退 Cargo.toml 版本
pub fn current_version() -> &'static str {
    parse_tag(option_env!("CLI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")))
}

#[derive(Parser)]
#[command(
    name = "cli",
    version = current_version(),
    about = "跨平台开发环境一键安装工具",
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// 显示版本信息
    Version,
    /// 列出已安装的工具与版本
    List,
    /// 交互式安装开发工具（无参数时弹出工具列表选择）
    Install {
        /// 工具名（不填则交互选择）
        tool: Option<String>,
    },
    /// 切换工具激活版本
    Use {
        /// 工具名
        tool: String,
        /// 目标版本（不填则交互选择）
        version: Option<String>,
    },
    /// 卸载已安装的工具版本（无参数时交互选择）
    Uninstall {
        /// 工具名（不填则交互选择）
        tool: Option<String>,
        /// 版本（不填则交互选择或卸载唯一版本）
        version: Option<String>,
    },
    /// 自更新：检查并升级到 GitHub Releases 最新版
    Update,
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Version => commands::version::run(),
        Command::List => commands::list::run(),
        Command::Install { tool } => commands::install::run(tool),
        Command::Use { tool, version } => commands::use_cmd::run(tool, version),
        Command::Uninstall { tool, version } => commands::uninstall::run(tool, version),
        Command::Update => commands::self_update::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn update_command_parses() {
        assert!(matches!(
            Cli::try_parse_from(["cli", "update"]).unwrap().command,
            Command::Update
        ));
    }

    #[test]
    fn old_self_update_name_rejected() {
        assert!(Cli::try_parse_from(["cli", "self-update"]).is_err());
    }
}
