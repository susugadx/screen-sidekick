#![forbid(unsafe_code)]

use std::{env, process};

use screen_sidekick_native_host::{
    cli_command_from_args, NativeHostCliCommand, NATIVE_HOST_CONFIG_SCHEMA_VERSION,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let command = cli_command_from_args(env::args());
    let caller_origin = match command {
        NativeHostCliCommand::PrintConfigSchemaVersion => {
            println!("{NATIVE_HOST_CONFIG_SCHEMA_VERSION}");
            return;
        }
        NativeHostCliCommand::Run { caller_origin } => caller_origin,
    };
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    if let Err(error) =
        screen_sidekick_native_host::run_from_environment(stdin, stdout, caller_origin).await
    {
        eprintln!("Screen Sidekick native host stopped: {error}");
        process::exit(1);
    }
}
