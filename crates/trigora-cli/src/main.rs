fn main() {
    if let Err(error) = trigora_cli::run() {
        trigora_cli::render_error(&error);
        std::process::exit(1);
    }
}
