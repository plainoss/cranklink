fn main() {
    if let Err(error) = cranklink::cli::execute(std::env::args().collect()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
