use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli) {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}
