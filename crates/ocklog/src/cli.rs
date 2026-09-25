use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "ocklog", author, version, about = "Modern Docker Log Viewer")]
pub struct Args {
    #[command(flatten)]
    pub common: CommonArgs,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(ClapArgs, Debug, Clone, Default)]
pub struct CommonArgs {
    #[arg(long, default_value_t = false)]
    pub flamegraph_enable: bool,

    #[arg(long)]
    pub flamegraph_save_file: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub log_enable: bool,

    #[arg(long)]
    pub log_save_path: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub log_console: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run the interactive log viewer (default)
    Run,
}
