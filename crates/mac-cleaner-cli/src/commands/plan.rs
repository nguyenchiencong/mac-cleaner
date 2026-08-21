use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;

use mac_cleaner_core::config::{ConfigProfile, WorkspaceRoot};
use mac_cleaner_core::model::plan::Plan;
use mac_cleaner_core::plan::PlanBuilder;
use mac_cleaner_core::scanners::{ScanContext, ScannerRegistry};

use crate::commands::{matches_category, parse_duration_to_days};
use crate::output::table::print_plan_table;

#[derive(Args, Debug)]
pub struct PlanArgs {
    /// Inactivity threshold for auto-approval (e.g. 30d, 14d, 60)
    #[arg(long = "older-than")]
    pub older_than: Option<String>,

    /// Filter scanners by category (e.g. rust, node, xcode, brew, cargo, npm, cache)
    #[arg(short = 'c', long = "category")]
    pub category: Option<String>,

    /// Save generated plan to file (e.g. plan.json)
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Additional workspace directory to scan
    #[arg(short = 'd', long = "search-dir")]
    pub search_dir: Option<PathBuf>,

    /// Output full plan in JSON format
    #[arg(long = "json")]
    pub json: bool,
}

pub fn execute_plan(args: PlanArgs) -> Result<()> {
    let config_path = ConfigProfile::default_config_path();
    let mut config = ConfigProfile::load_from_file(&config_path).unwrap_or_default();

    if let Some(dir) = args.search_dir {
        config.workspace_roots.push(WorkspaceRoot::new(dir));
    }

    if let Some(older_than_str) = &args.older_than {
        if let Some(days) = parse_duration_to_days(older_than_str) {
            config.age_threshold_days = Some(days);
        } else {
            bail!("Invalid --older-than format: '{}'. Expected e.g. 30d, 14d, 2w", older_than_str);
        }
    }

    let ctx = ScanContext::from_config(&config);
    let registry = ScannerRegistry::default_registry();

    let scan_results = if let Some(cat) = &args.category {
        registry.execute_filtered(&ctx, |scanner| matches_category(scanner.name(), cat))
    } else {
        registry.execute_all(&ctx)
    };

    let mut builder = PlanBuilder::new().with_auto_approve_safe(true);

    for res_wrapped in scan_results {
        match res_wrapped {
            Ok(res) => {
                builder = builder.add_scan_result(res);
            }
            Err(e) => {
                tracing::warn!("Scanner error during plan generation: {}", e);
            }
        }
    }

    let plan: Plan = builder.build();

    if let Some(out_path) = &args.output {
        let json_str = plan.to_json()?;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_path, json_str)?;
        println!(
            "{}",
            format!("Plan successfully written to: {}", out_path.display()).green().bold()
        );
        println!(
            "To execute this plan, run: {}",
            format!("mac-cleaner apply {}", out_path.display()).cyan()
        );
    } else if args.json {
        println!("{}", plan.to_json()?);
    } else {
        print_plan_table(&plan);
        println!(
            "\nTip: Save this plan with {} and run {}",
            "--output plan.json".yellow(),
            "mac-cleaner apply plan.json".cyan()
        );
    }

    Ok(())
}
