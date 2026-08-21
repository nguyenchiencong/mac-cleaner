use anyhow::Result;
use clap::Args;
use std::collections::HashSet;

use mac_cleaner_core::journal::JournalReader;
use mac_cleaner_core::model::candidate::AllocatedSize;
use mac_cleaner_core::model::manifest::JournalEntryKind;

use crate::commands::runs_dir;
use crate::output::json::{print_json, HistoryJsonOutput, HistoryJsonRow};
use crate::output::table::{print_history_table, HistoryRow};

#[derive(Args, Debug)]
pub struct HistoryArgs {
    /// Output history in JSON format
    #[arg(long = "json")]
    pub json: bool,
}

pub fn execute_history(args: HistoryArgs) -> Result<()> {
    let state_dir = runs_dir();
    let mut history_rows = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&state_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let filename_stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());

                let entries = match JournalReader::read_entries(&path) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                if entries.is_empty() {
                    continue;
                }

                let timestamp_str = entries
                    .first()
                    .map(|e| e.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "-".to_string());

                let mut action_ids = HashSet::new();
                let mut trash_count = 0usize;
                let mut external_count = 0usize;
                let mut reclaimed_bytes = 0u64;
                let mut had_external = false;

                for entry in &entries {
                    action_ids.insert(entry.action_id);

                    match &entry.kind {
                        JournalEntryKind::TrashSuccess => {
                            trash_count += 1;
                            if let Some(r) = &entry.receipt {
                                reclaimed_bytes = reclaimed_bytes.saturating_add(r.size.allocated_bytes);
                            }
                        }
                        JournalEntryKind::ExternalSuccess { .. } => {
                            external_count += 1;
                            had_external = true;
                        }
                        _ => {}
                    }
                }

                let status = if had_external {
                    "Irreversible (External)".to_string()
                } else if trash_count > 0 {
                    "Recoverable (Trash)".to_string()
                } else {
                    "No Changes".to_string()
                };

                history_rows.push(HistoryRow {
                    run_id: filename_stem,
                    timestamp: timestamp_str,
                    actions_count: action_ids.len(),
                    trashed_count: trash_count,
                    external_count,
                    reclaimed_allocated: AllocatedSize::format_human_bytes(reclaimed_bytes),
                    status,
                });
            }
        }
    }

    // Sort descending by timestamp / run_id
    history_rows.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    if args.json {
        let json_rows: Vec<HistoryJsonRow> = history_rows.iter().map(Into::into).collect();
        let out = HistoryJsonOutput {
            schema_version: 1,
            total_runs: json_rows.len(),
            runs: json_rows,
        };
        print_json(&out)?;
    } else {
        print_history_table(&history_rows);
    }

    Ok(())
}
