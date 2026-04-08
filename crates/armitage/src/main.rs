fn main() {
    if let Err(e) = armitage::cli::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
