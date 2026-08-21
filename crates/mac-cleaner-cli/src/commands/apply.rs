use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use dialoguer::Confirm;
use std::path::PathBuf;
use std::sync::Arc;

use mac_cleaner_core::apply::ApplyEngine;
use mac_cleaner_core::journal::JournalWriter;
use mac_cleaner_core::model::candidate::RevalidationFingerprint;
use mac_cleaner_core::model::plan::Plan;
use mac_cleaner_core::platform::process::ProcessExecutor;
use mac_cleaner_core::platform::trash::NativeTrashBackend;

use crate::commands::runs_dir;
use crate::output::json::{print_json, ApplyJsonOutput};
use crate::output::table::{print_apply_summary, print_plan_table};

#[derive(Args, Debug)]
pub struct ApplyArgs {
    /// Path to the plan file to execute (e.g. plan.json)
    #[arg(required = true)]
    pub plan_path: PathBuf,

    /// Skip interactive confirmation prompt
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,

    /// Drift handling behavior: 'skip' (default) or 'abort'
    #[arg(long = "on-drift", default_value = "skip")]
    pub on_drift: String,

    /// Output execution summary in JSON format
    #[arg(long = "json")]
    pub json: bool,
}

pub fn execute_apply(args: ApplyArgs) -> Result<()> {
    if !args.plan_path.exists() {
        bail!("Plan file not found at: {}", args.plan_path.display());
    }

    let plan_json = std::fs::read_to_string(&args.plan_path)
        .with_context(|| format!("Failed to read plan file at {}", args.plan_path.display()))?;

    let plan = Plan::from_json(&plan_json)
        .with_context(|| "Failed to parse plan JSON: invalid schema or corrupted file")?;

    // 1. Verify cryptographic canonical hash
    if !plan.verify_hash() {
        bail!(
            "Plan hash verification failed for {}: plan contents have been modified or corrupted!",
            plan.metadata.plan_id
        );
    }

    let approved_count = plan.approved_actions().count();
    if approved_count == 0 {
        println!(
            "{}",
            "Plan contains 0 approved actions. Nothing to execute.".yellow().bold()
        );
        return Ok(());
    }

    if !args.json && !args.yes {
        print_plan_table(&plan);
        println!();

        let proceed = Confirm::new()
            .with_prompt(format!(
                "Proceed with executing {} approved action(s)?",
                approved_count
            ))
            .default(false)
            .interact()?;

        if !proceed {
            println!("{}", "Execution aborted by user.".yellow());
            return Ok(());
        }
    }

    // Handle --on-drift abort: verify all fingerprints before performing any mutation
    if args.on_drift.to_lowercase() == "abort" {
        for action in plan.approved_actions() {
            if action.execution_strategy.is_reversible() {
                match RevalidationFingerprint::from_path(&action.path) {
                    Ok(live_fp) => {
                        if !action.fingerprint.matches(&live_fp) {
                            bail!(
                                "Aborting execution (--on-drift=abort): pre-flight drift detected for {}",
                                action.path.display()
                            );
                        }
                    }
                    Err(e) => {
                        bail!(
                            "Aborting execution (--on-drift=abort): path {} vanished or inaccessible ({})",
                            action.path.display(),
                            e
                        );
                    }
                }
            }
        }
    }

    // 2. Set up Run Journal at ~/.local/state/mac-cleaner/runs/run_<timestamp>_<uuid>.jsonl
    let state_dir = runs_dir();
    let mut journal = JournalWriter::create_default_in(&state_dir)
        .with_context(|| format!("Failed to create journal in {}", state_dir.display()))?;

    let journal_path = journal.path().to_path_buf();

    // Extract run identifier from journal filename
    let run_id = journal_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| plan.metadata.plan_id.to_string());

    // 3. Execute via ApplyEngine
    let trash_backend = Arc::new(NativeTrashBackend::new());
    let process_executor = Arc::new(ProcessExecutor::new());
    let engine = ApplyEngine::new(trash_backend, process_executor);

    let summary = engine.apply(&plan, &mut journal)?;

    if args.json {
        let out = ApplyJsonOutput::new(&summary, &journal_path.display().to_string());
        print_json(&out)?;
    } else {
        print_apply_summary(&summary, &journal_path);
        println!(
            "\nManifest ID: {}",
            run_id.bold().cyan()
        );
        println!(
            "To restore items from this execution run, execute:\n  {}",
            format!("mac-cleaner restore {}", run_id).green().bold()
        );
    }

    Ok(())
}
