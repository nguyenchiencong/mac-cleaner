use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;

use mac_cleaner_core::config::ConfigProfile;

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Initialize default configuration file (~/.config/mac-cleaner/config.toml)
    Init {
        /// Overwrite existing configuration file if present
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    /// Show current active configuration
    Show,
}

pub fn execute_config(args: ConfigArgs) -> Result<()> {
    let config_path = ConfigProfile::default_config_path();

    match args.command {
        ConfigCommand::Init { force } => {
            match ConfigProfile::init_file(&config_path, force) {
                Ok(_) => {
                    println!(
                        "{}",
                        format!(
                            "Initialized configuration file at: {}",
                            config_path.display()
                        )
                        .green()
                        .bold()
                    );
                }
                Err(mac_cleaner_core::config::ConfigError::AlreadyExists(_)) => {
                    println!(
                        "{}",
                        format!(
                            "Configuration file already exists at {}. Use --force to overwrite.",
                            config_path.display()
                        )
                        .yellow()
                    );
                }
                Err(e) => return Err(e.into()),
            }
        }
        ConfigCommand::Show => {
            if config_path.exists() {
                println!(
                    "{}",
                    format!("Active Configuration ({})", config_path.display())
                        .bold()
                        .underline()
                );
                let content = std::fs::read_to_string(&config_path)?;
                println!("\n{}", content);
            } else {
                println!(
                    "{}",
                    format!(
                        "No configuration file found at {}. Showing defaults:",
                        config_path.display()
                    )
                    .dimmed()
                );
                let default_cfg = ConfigProfile::default();
                let toml_str = toml::to_string_pretty(&default_cfg)?;
                println!("\n{}", toml_str);
                println!(
                    "\nTip: Run '{}' to persist default configuration.",
                    "mac-cleaner config init".cyan()
                );
            }
        }
    }

    Ok(())
}
