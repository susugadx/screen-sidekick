#![forbid(unsafe_code)]

use std::{env, process};

use screen_sidekick_sidekick_daemon::run_stdio_status_daemon;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.as_slice() != ["--stdio-status"] {
        eprintln!("Usage: screen-sidekick-daemon --stdio-status");
        process::exit(2);
    }

    if let Err(error) = run_stdio_status_daemon(tokio::io::stdin(), tokio::io::stdout()).await {
        eprintln!("Screen Sidekick daemon stopped: {error}");
        process::exit(1);
    }
}
