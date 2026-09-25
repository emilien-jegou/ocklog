use clap::Parser;
use std::error::Error;
use std::process::ExitCode;

mod ansi;
mod cli;
mod commands;
mod commons;
mod config;
mod docker;
mod query;
mod state;
mod terminal_colors;
mod ui;
mod worker;

use cli::Args;
use commands::RunOptions;

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    let args = Args::parse();
    let color_mode = terminal_colors::detect_color_mode()?;

    let _trace_guard = commons::tracing::Tracer::builder()
        .flamegraph_enable(args.common.flamegraph_enable)
        .flamegraph_save_file(args.common.flamegraph_save_file.clone())
        .log_enable(args.common.log_enable)
        .log_save_path(args.common.log_save_path.clone())
        .log_console(args.common.log_console)
        .build()
        .setup()?;

    tracing::info!("Starting ocklog...");

    if let Err(e) = commands::run(RunOptions { args, color_mode }).await {
        tracing::error!("Application error: {:?}", e);
        eprintln!("Error: {}", e);
        return Ok(ExitCode::FAILURE);
    }

    config::clear_registry();
    Ok(ExitCode::SUCCESS)
}
