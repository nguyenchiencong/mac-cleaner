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
                let mut purged_count = 0usize;
                let mut reclaimed_bytes = 0u64;

                for entry in &entries {
                    action_ids.insert(entry.action_id);

                    match &entry.kind {
                        JournalEntryKind::TrashSuccess => {
                            trash_count += 1;
                            if let Some(r) = &entry.receipt {
                                reclaimed_bytes =
                                    reclaimed_bytes.saturating_add(r.size.allocated_bytes);
                            }
                        }
                        JournalEntryKind::ExternalSuccess { .. } => {
                            external_count += 1;
                        }
                        JournalEntryKind::PurgeSuccess => {
                            purged_count += 1;
                        }
                        _ => {}
                    }
                }

                let status = history_status(trash_count, external_count, purged_count).to_string();

                history_rows.push(HistoryRow {
                    run_id: filename_stem,
                    timestamp: timestamp_str,
                    actions_count: action_ids.len(),
                    trashed_count: trash_count,
                    external_count,
                    purged_count,
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

fn history_status(trash_count: usize, external_count: usize, purged_count: usize) -> &'static str {
    let has_trash = trash_count > 0;
    let has_irreversible_actions = external_count > 0 || purged_count > 0;

    match (has_trash, has_irreversible_actions) {
        (true, true) => "Mixed (Trash + Irreversible)",
        (false, true) => "Irreversible (External or In-Place Purge)",
        (true, false) => "Recoverable (Trash)",
        (false, false) => "No Changes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_status_preserves_purge_irreversibility() {
        assert_eq!(history_status(0, 0, 1), "Irreversible (External or In-Place Purge)");
        assert_eq!(history_status(1, 0, 1), "Mixed (Trash + Irreversible)");
        assert_eq!(history_status(1, 0, 0), "Recoverable (Trash)");
    }
}
