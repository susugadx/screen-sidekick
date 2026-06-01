fn main() {
    if let Err(error) = screen_sidekick_desktop::run() {
        eprintln!("failed to run Screen Sidekick: {error}");
        std::process::exit(1);
    }
}
