fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = if args.is_empty() { None } else { Some(args) };
    opensession_tui::run(paths)
}
