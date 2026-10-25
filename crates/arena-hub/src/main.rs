use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    arena_hub::start(std::env::args_os().skip(1)).await
}
