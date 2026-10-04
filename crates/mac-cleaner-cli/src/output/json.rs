use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::output::table::HistoryRow;
use mac_cleaner_core::apply::{ApplyItemResult, ApplySummary};
use mac_cleaner_core::model::candidate::Candidate;
use mac_cleaner_core::model::diagnostic::{CoverageStatus, ScanDiagnostic};
use mac_cleaner_core::restore::RestoreSummary;

pub const CURRENT_CLI_JSON_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanJsonOutput {
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub total_candidates: usize,
    pub total_allocated_bytes: u64,
    pub total_logical_bytes: u64,
    pub coverage_status: CoverageStatus,
    pub candidates: Vec<Candidate>,
    pub diagnostics: Vec<ScanDiagnostic>,
}

impl ScanJsonOutput {
    pub fn new(
        candidates: Vec<Candidate>,
        coverage_status: CoverageStatus,
        diagnostics: Vec<ScanDiagnostic>,
    ) -> Self {
        let mut total_allocated_bytes = 0u64;
        let mut total_logical_bytes = 0u64;

        for c in &candidates {
            total_allocated_bytes = total_allocated_bytes.saturating_add(c.size.allocated_bytes);
            total_logical_bytes = total_logical_bytes.saturating_add(c.size.logical_bytes);
        }

        Self {
            schema_version: CURRENT_CLI_JSON_SCHEMA_VERSION,
            timestamp: Utc::now(),
            total_candidates: candidates.len(),
            total_allocated_bytes,
            total_logical_bytes,
            coverage_status,
            candidates,
            diagnostics,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyJsonOutput {
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub journal_path: String,
    pub total_processed: usize,
    pub successful_trash_moves: usize,
    pub successful_external_commands: usize,
    pub successful_in_place_purges: usize,
    pub skipped_drifts: usize,
    pub failures: usize,
    pub reclaimed_allocated_bytes: u64,
    pub reclaimed_logical_bytes: u64,
    pub observed_free_space_delta_bytes: Option<i64>,
    pub item_results: Vec<ApplyItemResult>,
}

impl ApplyJsonOutput {
    pub fn new(summary: &ApplySummary, journal_path: &str) -> Self {
        Self {
            schema_version: CURRENT_CLI_JSON_SCHEMA_VERSION,
            timestamp: Utc::now(),
            journal_path: journal_path.to_string(),
            total_processed: summary.total_processed,
            successful_trash_moves: summary.successful_trash_moves,
            successful_external_commands: summary.successful_external_commands,
            successful_in_place_purges: summary.successful_in_place_purges,
            skipped_drifts: summary.skipped_drifts,
            failures: summary.failures,
            reclaimed_allocated_bytes: summary.reclaimed_size.allocated_bytes,
            reclaimed_logical_bytes: summary.reclaimed_size.logical_bytes,
            observed_free_space_delta_bytes: summary.observed_free_space_delta_bytes,
            item_results: summary.item_results.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreJsonOutput {
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub run_id: String,
    pub total_attempted: usize,
    pub successfully_restored: usize,
    pub skipped_collisions: usize,
    pub not_found_in_trash: usize,
    pub failed: usize,
    pub item_results: Vec<mac_cleaner_core::restore::RestoreItemResult>,
}

impl RestoreJsonOutput {
    pub fn new(run_id: &str, summary: &RestoreSummary) -> Self {
        Self {
            schema_version: CURRENT_CLI_JSON_SCHEMA_VERSION,
            timestamp: Utc::now(),
            run_id: run_id.to_string(),
            total_attempted: summary.total_attempted,
            successfully_restored: summary.successfully_restored,
            skipped_collisions: summary.skipped_collisions,
            not_found_in_trash: summary.not_found_in_trash,
            failed: summary.failed,
            item_results: summary.item_results.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryJsonOutput {
    pub schema_version: u32,
    pub total_runs: usize,
    pub runs: Vec<HistoryJsonRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryJsonRow {
    pub run_id: String,
    pub timestamp: String,
    pub actions_count: usize,
    pub trashed_count: usize,
    pub external_count: usize,
    pub purged_count: usize,
    pub reclaimed_allocated: String,
    pub status: String,
}

impl From<&HistoryRow> for HistoryJsonRow {
    fn from(r: &HistoryRow) -> Self {
        Self {
            run_id: r.run_id.clone(),
            timestamp: r.timestamp.clone(),
            actions_count: r.actions_count,
            trashed_count: r.trashed_count,
            external_count: r.external_count,
            purged_count: r.purged_count,
            reclaimed_allocated: r.reclaimed_allocated.clone(),
            status: r.status.clone(),
        }
    }
}

/// Helper function to serialize any value to JSON stdout.
pub fn print_json<T: Serialize>(val: &T) -> Result<(), serde_json::Error> {
    let s = serde_json::to_string_pretty(val)?;
    println!("{}", s);
    Ok(())
}
