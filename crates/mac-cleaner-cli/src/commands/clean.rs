use anyhow::Result;
use clap::Args;
use colored::Colorize;
use dialoguer::{Confirm, MultiSelect};
use is_terminal::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use mac_cleaner_core::apply::ApplyEngine;
use mac_cleaner_core::config::{ConfigProfile, WorkspaceRoot};
use mac_cleaner_core::journal::JournalWriter;
use mac_cleaner_core::model::candidate::Candidate;
use mac_cleaner_core::model::diagnostic::CoverageStatus;
use mac_cleaner_core::model::plan::{Plan, PlannedAction};
use mac_cleaner_core::platform::process::ProcessExecutor;
use mac_cleaner_core::platform::trash::NativeTrashBackend;
use mac_cleaner_core::scanners::{ScanContext, ScannerRegistry};

use crate::commands::{matches_category, parse_duration_to_days, runs_dir};
use crate::output::table::{print_apply_summary, print_diagnostics_summary};

#[derive(Args, Debug)]
pub struct CleanArgs {
    /// Skip interactive prompts and auto-approve safe items
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,

    /// Filter scanners by category (e.g. rust, node, xcode, brew, cargo, npm, cache)
    #[arg(short = 'c', long = "category")]
    pub category: Option<String>,

    /// Additional workspace directory to scan
    #[arg(short = 'd', long = "search-dir")]
    pub search_dir: Option<PathBuf>,

    /// Inactivity threshold for auto-approval (e.g. 30d, 14d)
    #[arg(long = "older-than")]
    pub older_than: Option<String>,
}

pub fn execute_clean(args: CleanArgs) -> Result<()> {
    println!("{}", "🔍 Scanning system and configured workspaces...".bold());

    let config_path = ConfigProfile::default_config_path();
    let mut config = ConfigProfile::load_from_file(&config_path).unwrap_or_default();

    if let Some(dir) = args.search_dir {
        config.workspace_roots.push(WorkspaceRoot::new(dir));
    }

    if let Some(older_than_str) = &args.older_than {
        if let Some(days) = parse_duration_to_days(older_than_str) {
            config.age_threshold_days = Some(days);
        }
    }

    let ctx = ScanContext::from_config(&config);
    let registry = ScannerRegistry::default_registry();

    let scan_results = if let Some(cat) = &args.category {
        registry.execute_filtered(&ctx, |scanner| matches_category(scanner.name(), cat))
    } else {
        registry.execute_all(&ctx)
    };

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut merged_coverage = CoverageStatus::Complete;

    for res_wrapped in scan_results {
        match res_wrapped {
            Ok(res) => {
                candidates.extend(res.candidates);
                diagnostics.extend(res.diagnostics);
                merged_coverage = merged_coverage.merge(res.coverage_status);
            }
            Err(e) => {
                tracing::warn!("Scanner error during clean: {}", e);
            }
        }
    }

    if candidates.is_empty() {
        println!(
            "{}",
            "✨ Your system is already clean! No reclaimable candidates found."
                .green()
                .bold()
        );
        print_diagnostics_summary(&diagnostics);
        return Ok(());
    }

    print_diagnostics_summary(&diagnostics);

    let is_interactive = std::io::stdin().is_terminal() && !args.yes;

    let selected_indices: Vec<usize> = if is_interactive {
        println!(
            "\n{}",
            format!(
                "Found {} candidates. Select items to reclaim (Space to toggle, Enter to confirm):",
                candidates.len()
            )
            .bold()
        );

        let items: Vec<String> = candidates
            .iter()
            .map(|c| {
                let tier = match c.risk_tier {
                    mac_cleaner_core::model::candidate::RiskTier::Tier1ReDownloadable => "[Tier 1]",
                    mac_cleaner_core::model::candidate::RiskTier::Tier2Rebuildable => "[Tier 2]",
                    mac_cleaner_core::model::candidate::RiskTier::Tier3Retained => "[Tier 3]",
                    mac_cleaner_core::model::candidate::RiskTier::Tier4UserData => "[Tier 4]",
                };
                let display_target = if let Some(cmd) = c.metadata.get("command") {
                    format!("Command: {}", cmd)
                } else {
                    c.path.display().to_string()
                };
                format!(
                    "{} {:<10} {} ({})",
                    tier,
                    c.size.human_readable_allocated(),
                    display_target,
                    c.description
                )
            })
            .collect();

        let defaults: Vec<bool> = candidates
            .iter()
            .map(|c| {
                if merged_coverage.is_degraded() {
                    false
                } else {
                    c.metadata
                        .get("safe_for_auto_clean")
                        .map(|v| v == "true")
                        .unwrap_or_else(|| c.risk_tier.is_safe_for_auto_clean())
                }
            })
            .collect();

        MultiSelect::new()
            .items(&items)
            .defaults(&defaults)
            .interact()?
    } else {
        // Non-interactive or --yes: select all safe candidates
        candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let safe = if merged_coverage.is_degraded() {
                    false
                } else {
                    c.metadata
                        .get("safe_for_auto_clean")
                        .map(|v| v == "true")
                        .unwrap_or_else(|| c.risk_tier.is_safe_for_auto_clean())
                };
                if safe {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    };

    if selected_indices.is_empty() {
        println!("{}", "No candidates selected for reclamation.".yellow());
        return Ok(());
    }

    // Build Plan with selected candidates approved
    let actions: Vec<PlannedAction> = candidates
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let approved = selected_indices.contains(&i);
            PlannedAction::from_candidate(&c, approved)
        })
        .collect();

    let plan = Plan::new(actions, merged_coverage, diagnostics);
    let approved_size = plan.total_approved_size();
    let approved_count = plan.approved_actions().count();

    println!(
        "\n{}",
        format!(
            "Ready to reclaim {} across {} item(s).",
            approved_size.human_readable_allocated().green().bold(),
            approved_count
        )
        .bold()
    );

    if is_interactive {
        let proceed = Confirm::new()
            .with_prompt("Do you want to proceed with reclamation now?")
            .default(true)
            .interact()?;

        if !proceed {
            println!("{}", "Reclamation cancelled by user.".yellow());
            return Ok(());
        }
    }

    // Execute plan
    let state_dir = runs_dir();
    let mut journal = JournalWriter::create_default_in(&state_dir)?;
    let journal_path = journal.path().to_path_buf();
    let run_id = journal_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| plan.metadata.plan_id.to_string());

    let trash_backend = Arc::new(NativeTrashBackend::new());
    let process_executor = Arc::new(ProcessExecutor::new());
    let engine = ApplyEngine::new(trash_backend, process_executor);

    println!("{}", "🚀 Processing reclamation...".bold());
    let summary = engine.apply(&plan, &mut journal)?;

    print_apply_summary(&summary, &journal_path);

    println!(
        "\n{}",
        format!("✨ Reclamation Complete! Manifest ID: {}", run_id)
            .green()
            .bold()
    );
    println!(
        "To undo this operation, run:\n  {}",
        format!("mac-cleaner restore {}", run_id).cyan().bold()
    );

    Ok(())
}
