use clap::Parser;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if cli::wants_top_level_help(&args) {
        let help = cli::render_top_help();
        // 无参数保持 clap 既有语义：stderr + exit(2)；-h/--help/help 走 stdout
        if args.is_empty() {
            eprint!("{help}");
            std::process::exit(2);
        }
        print!("{help}");
        return;
    }
    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli) {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}
