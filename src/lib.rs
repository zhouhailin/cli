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
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Version => {
            println!("cli {VERSION}");
            Ok(())
        }
        Command::List => commands::list::run(),
    }
}
