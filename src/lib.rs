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
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}",
    infer_subcommands = true
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
        /// 工具名（不填则交互选择已安装工具）
        tool: Option<String>,
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
    /// 操作系统镜像查询与下载（阿里云镜像）
    Os {
        #[command(subcommand)]
        subcommand: OsCommand,
    },
}

#[derive(Subcommand)]
pub enum OsCommand {
    /// 列出阿里云镜像支持的系统名
    List,
    /// 查询系统全部镜像（版本/大小/链接）
    Info {
        /// 系统名（如 almalinux、ubuntu）
        name: String,
    },
    /// 下载系统 ISO 镜像
    Download {
        /// 系统名（如 almalinux、ubuntu）
        name: String,
        /// 精确指定镜像版本（version 字段）；不填则交互选择
        #[arg(long)]
        version: Option<String>,
        /// 下载保存目录（默认当前目录）
        #[arg(short, long, default_value = ".")]
        output_dir: String,
    },
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Version => commands::version::run(),
        Command::List => commands::list::run(),
        Command::Install { tool } => commands::install::run(tool),
        Command::Use { tool, version } => commands::use_cmd::run(tool, version),
        Command::Uninstall { tool, version } => commands::uninstall::run(tool, version),
        Command::Update => commands::self_update::run(),
        Command::Os { subcommand } => commands::os::run(subcommand),
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

    #[test]
    fn prefix_abbreviations_parse() {
        // 唯一前缀推断：cli i / cli l / cli v / cli up / cli un
        assert!(matches!(
            Cli::try_parse_from(["cli", "i"]).unwrap().command,
            Command::Install { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "ins"]).unwrap().command,
            Command::Install { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "l"]).unwrap().command,
            Command::List
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "v"]).unwrap().command,
            Command::Version
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "up"]).unwrap().command,
            Command::Update
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "un"]).unwrap().command,
            Command::Uninstall { .. }
        ));
    }

    #[test]
    fn ambiguous_prefix_rejected() {
        // u 同时是 use/uninstall/update 的前缀 → 歧义报错
        assert!(Cli::try_parse_from(["cli", "u"]).is_err());
    }

    #[test]
    fn full_command_names_still_parse() {
        // 完整命令名回归确认
        assert!(matches!(
            Cli::try_parse_from(["cli", "use"]).unwrap().command,
            Command::Use { .. }
        ));
    }

    #[test]
    fn os_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["cli", "os", "list"]).unwrap().command,
            Command::Os {
                subcommand: OsCommand::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "os", "info", "almalinux"])
                .unwrap()
                .command,
            Command::Os {
                subcommand: OsCommand::Info { .. }
            }
        ));
        // 前缀缩写：o → os，l → list，i → info，d → download
        assert!(matches!(
            Cli::try_parse_from(["cli", "o", "l"]).unwrap().command,
            Command::Os {
                subcommand: OsCommand::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "o", "d", "ubuntu", "--version", "x", "-o", "/tmp"])
                .unwrap()
                .command,
            Command::Os {
                subcommand: OsCommand::Download { .. }
            }
        ));
    }
}
