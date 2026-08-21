use clap::{Parser, Subcommand};
use colored::Colorize;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod commands;
pub mod output;

use commands::apply::{execute_apply, ApplyArgs};
use commands::clean::{execute_clean, CleanArgs};
use commands::config::{execute_config, ConfigArgs};
use commands::history::{execute_history, HistoryArgs};
use commands::plan::{execute_plan, PlanArgs};
use commands::restore::{execute_restore, RestoreArgs};
use commands::scan::{execute_scan, ScanArgs};

#[derive(Parser, Debug)]
#[command(
    name = "mac-cleaner",
    author = "macOS Cleaner Team",
    version,
    about = "Safe, verifiable, and recoverable storage reclamation CLI for macOS",
    long_about = "A high-assurance developer storage cleaner for macOS that discovers, plans, safely relocates to Trash, and restores project build caches and simulator data."
)]
struct Cli {
    /// Verbose logging output
    #[arg(short = 'v', long = "verbose", global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Discover reclaimable storage candidates across the system and workspaces
    Scan(ScanArgs),

    /// Generate an immutable, reviewable reclamation plan with safety classification
    Plan(PlanArgs),

    /// Execute an approved reclamation plan with pre-flight revalidation
    Apply(ApplyArgs),

    /// Recover and restore previously cleaned items using audit journals
    Restore(RestoreArgs),

    /// View history and audit records of previous reclamation execution runs
    History(HistoryArgs),

    /// Manage configuration profile (~/.config/mac-cleaner/config.toml)
    Config(ConfigArgs),

    /// Interactive scan, review, and reclamation workflow
    Clean(CleanArgs),
}

fn init_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("debug,mac_cleaner_core=debug,mac_cleaner_cli=debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false).without_time())
        .try_init();
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    match cli.command {
        Some(Commands::Scan(args)) => execute_scan(args),
        Some(Commands::Plan(args)) => execute_plan(args),
        Some(Commands::Apply(args)) => execute_apply(args),
        Some(Commands::Restore(args)) => execute_restore(args),
        Some(Commands::History(args)) => execute_history(args),
        Some(Commands::Config(args)) => execute_config(args),
        Some(Commands::Clean(args)) => execute_clean(args),
        None => {
            // Default to interactive clean if invoked without subcommands
            execute_clean(CleanArgs {
                yes: false,
                category: None,
                search_dir: None,
                older_than: None,
            })
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("\n{} {}", "Error:".red().bold(), err);
        std::process::exit(1);
    }
}
