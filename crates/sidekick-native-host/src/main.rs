#![forbid(unsafe_code)]

use std::{env, process};

use screen_sidekick_native_host::{caller_origin_from_args, run_from_environment};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let caller_origin = caller_origin_from_args(env::args());
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    if let Err(error) = run_from_environment(stdin, stdout, caller_origin).await {
        eprintln!("Screen Sidekick native host stopped: {error}");
        process::exit(1);
    }
}
