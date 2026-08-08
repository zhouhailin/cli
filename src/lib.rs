pub mod commands;
pub mod core;

use clap::{Parser, Subcommand};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "cli", version = VERSION, about = "跨平台开发环境一键安装工具")]
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
    /// 交互式安装开发工具（java/node/go/maven/redis/mysql）
    Install {
        /// 工具名
        tool: String,
    },
    /// 切换工具激活版本
    Use {
        /// 工具名
        tool: String,
        /// 目标版本（不填则交互选择）
        version: Option<String>,
    },
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Version => commands::version::run(),
        Command::List => commands::list::run(),
        Command::Install { tool } => commands::install::run(tool),
        Command::Use { tool, version } => commands::use_cmd::run(tool, version),
    }
}
