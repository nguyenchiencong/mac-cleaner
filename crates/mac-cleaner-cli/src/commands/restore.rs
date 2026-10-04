use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use mac_cleaner_core::journal::JournalReader;
use mac_cleaner_core::platform::trash::NativeTrashBackend;
use mac_cleaner_core::restore::RestoreEngine;

use crate::commands::runs_dir;
use crate::output::json::{print_json, RestoreJsonOutput};
use crate::output::table::print_restore_summary;

#[derive(Args, Debug)]
pub struct RestoreArgs {
    /// Run ID, manifest filename, or journal path to restore
    #[arg(required = true)]
    pub run_id: String,

    /// Restore all recoverable items in this run
    #[arg(long = "all", default_value = "true")]
    pub all: bool,

    /// Specific Candidate or Receipt ID to restore
    #[arg(long = "candidate-id")]
    pub candidate_id: Option<Uuid>,

    /// Output restoration summary in JSON format
    #[arg(long = "json")]
    pub json: bool,
}

pub fn find_journal_file(run_id_or_path: &str) -> Option<PathBuf> {
    let direct_path = PathBuf::from(run_id_or_path);
    if direct_path.exists() {
        return Some(direct_path);
    }

    let runs = runs_dir();
    let direct_in_runs = runs.join(run_id_or_path);
    if direct_in_runs.exists() {
        return Some(direct_in_runs);
    }
    let with_jsonl = runs.join(format!("{}.jsonl", run_id_or_path));
    if with_jsonl.exists() {
        return Some(with_jsonl);
    }

    // Prefix / Substring match in runs_dir
    if let Ok(entries) = std::fs::read_dir(&runs) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(run_id_or_path) && name.ends_with(".jsonl") {
                return Some(entry.path());
            }
        }
    }

    None
}

pub fn execute_restore(args: RestoreArgs) -> Result<()> {
    let journal_path = match find_journal_file(&args.run_id) {
        Some(p) => p,
        None => {
            bail!(
                "Could not find run journal matching '{}' in {}",
                args.run_id,
                runs_dir().display()
            );
        }
    };

    let receipts = JournalReader::read_trash_receipts(&journal_path)?;

    if receipts.is_empty() {
        if args.json {
            print_json(&RestoreJsonOutput::new(
                &args.run_id,
                &mac_cleaner_core::restore::RestoreSummary::default(),
            ))?;
        } else {
            println!(
                "{}",
                "No restorable trash receipts found in this run journal.".yellow()
            );
        }
        return Ok(());
    }

    let target_receipts: Vec<_> = if let Some(target_id) = args.candidate_id {
        let filtered: Vec<_> = receipts
            .into_iter()
            .filter(|r| r.receipt_id == target_id)
            .collect();
        if filtered.is_empty() {
            bail!(
                "Candidate / Receipt ID '{}' not found in run {}",
                target_id,
                args.run_id
            );
        }
        filtered
    } else {
        receipts
    };

    let trash_backend = Arc::new(NativeTrashBackend::new());
    let restore_engine = RestoreEngine::new(trash_backend);

    if !args.json {
        println!(
            "Restoring {} item(s) from run journal at {}...",
            target_receipts.len(),
            journal_path.display().to_string().cyan()
        );
    }

    let summary = restore_engine.restore_receipts(&target_receipts);

    if args.json {
        let out = RestoreJsonOutput::new(&args.run_id, &summary);
        print_json(&out)?;
    } else {
        print_restore_summary(&summary);
        if summary.successfully_restored > 0 {
            println!(
                "{}",
                format!(
                    "Successfully restored {} item(s) to original locations.",
                    summary.successfully_restored
                )
                .green()
                .bold()
            );
        }
        if summary.skipped_collisions > 0 {
            println!(
                "{}",
                format!(
                    "Notice: {} item(s) skipped due to destination file collisions.",
                    summary.skipped_collisions
                )
                .yellow()
            );
        }
    }

    Ok(())
}
