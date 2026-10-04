fn main() {
    if let Err(error) = trigora_cli::run() {
        if error.raw {
            if let Some(message) = &error.message {
                eprint!("{message}");
            }
        } else {
            trigora_cli::render_error(&error);
        }
        std::process::exit(error.code);
    }
}
