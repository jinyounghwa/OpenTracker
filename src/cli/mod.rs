pub mod onboard;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "OpenTracker",
    about = "PC Activity Intelligence & Report System"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Onboard {
        #[arg(long, default_value_t = false)]
        install_daemon: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Status,
    Doctor,
    Start,
    Stop,
    Restart,
    Dashboard,
    Report {
        #[arg(long)]
        date: Option<String>,
    },
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },
    Service,
    Update,
    Uninstall,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    Set { key: String, value: String },
    Get { key: String },
}

#[derive(Debug, Subcommand)]
pub enum AiCommands {
    Test {
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
}
