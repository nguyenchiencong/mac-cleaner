use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use mac_cleaner_core::config::{ConfigProfile, WorkspaceRoot};
use mac_cleaner_core::model::diagnostic::{CoverageStatus, ScanDiagnostic};
use mac_cleaner_core::scanners::{ScanContext, ScannerRegistry};

use crate::commands::matches_category;
use crate::commands::progress::ScanProgress;
use crate::output::json::{print_json, ScanJsonOutput};
use crate::output::table::print_candidates_table;

#[derive(Args, Debug)]
pub struct ScanArgs {
    /// Output format: 'text' or 'json'
    #[arg(short = 'f', long = "format", default_value = "text")]
    pub format: String,

    /// Shortcut for JSON output format
    #[arg(long = "json")]
    pub json: bool,

    /// Additional workspace directory to scan
    #[arg(short = 'd', long = "search-dir")]
    pub search_dir: Option<PathBuf>,

    /// Filter scanners by category (e.g. rust, node, xcode, brew, cargo, npm, cache)
    #[arg(short = 'c', long = "category")]
    pub category: Option<String>,
}

pub fn execute_scan(args: ScanArgs) -> Result<()> {
    let config_path = ConfigProfile::default_config_path();
    let mut config = ConfigProfile::load_from_file(&config_path).unwrap_or_default();

    if let Some(dir) = args.search_dir {
        config.workspace_roots.push(WorkspaceRoot::new(dir));
    }

    let ctx = ScanContext::from_config(&config);
    let registry = ScannerRegistry::default_registry();

    let scan_progress = ScanProgress::start("Scanning configured workspaces and caches...");
    let scan_results = if let Some(cat) = &args.category {
        registry.execute_filtered(&ctx, |scanner| matches_category(scanner.name(), cat))
    } else {
        registry.execute_all(&ctx)
    };
    drop(scan_progress);

    let mut all_candidates = Vec::new();
    let mut all_diagnostics = Vec::new();
    let mut merged_coverage = CoverageStatus::Complete;

    for res_wrapped in scan_results {
        match res_wrapped {
            Ok(res) => {
                all_candidates.extend(res.candidates);
                all_diagnostics.extend(res.diagnostics);
                merged_coverage = merged_coverage.merge(res.coverage_status);
            }
            Err(e) => {
                tracing::warn!("Scanner execution warning: {}", e);
                all_diagnostics.push(ScanDiagnostic::incomplete_coverage(
                    None,
                    "scanner",
                    e.to_string(),
                ));
                merged_coverage = CoverageStatus::Incomplete;
            }
        }
    }

    let use_json = args.json || args.format.to_lowercase() == "json";

    if use_json {
        let output = ScanJsonOutput::new(all_candidates, merged_coverage, all_diagnostics);
        print_json(&output)?;
    } else {
        print_candidates_table(&all_candidates, merged_coverage, &all_diagnostics);
    }

    Ok(())
}
