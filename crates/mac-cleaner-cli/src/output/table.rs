use colored::Colorize;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::path::Path;

use mac_cleaner_core::apply::{ApplyItemStatus, ApplySummary};
use mac_cleaner_core::model::candidate::{AllocatedSize, Candidate, RiskTier};
use mac_cleaner_core::model::diagnostic::{CoverageStatus, ScanDiagnostic};
use mac_cleaner_core::model::plan::Plan;
use mac_cleaner_core::restore::{RestoreItemStatus, RestoreSummary};

pub fn risk_tier_color(tier: RiskTier) -> Color {
    match tier {
        RiskTier::Tier1ReDownloadable => Color::Green,
        RiskTier::Tier2Rebuildable => Color::Cyan,
        RiskTier::Tier3Retained => Color::Yellow,
        RiskTier::Tier4UserData => Color::Magenta,
    }
}

pub fn risk_tier_colored_str(tier: RiskTier) -> colored::ColoredString {
    match tier {
        RiskTier::Tier1ReDownloadable => tier.display_name().green(),
        RiskTier::Tier2Rebuildable => tier.display_name().cyan(),
        RiskTier::Tier3Retained => tier.display_name().yellow(),
        RiskTier::Tier4UserData => tier.display_name().magenta(),
    }
}

/// Formats and prints discovered scan candidates.
pub fn print_candidates_table(
    candidates: &[Candidate],
    coverage_status: CoverageStatus,
    diagnostics: &[ScanDiagnostic],
) {
    if candidates.is_empty() {
        println!("{}", "No reclaimable candidates found.".green().bold());
        print_diagnostics_summary(diagnostics);
        return;
    }

    println!(
        "\n{}",
        format!("Discovered {} Candidates", candidates.len())
            .bold()
            .underline()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("#").add_attribute(Attribute::Bold),
            Cell::new("Risk Tier").add_attribute(Attribute::Bold),
            Cell::new("Scanner").add_attribute(Attribute::Bold),
            Cell::new("Allocated").add_attribute(Attribute::Bold),
            Cell::new("Logical").add_attribute(Attribute::Bold),
            Cell::new("Path / Target").add_attribute(Attribute::Bold),
        ]);

    let mut total_allocated = 0u64;
    let mut total_logical = 0u64;

    for (idx, candidate) in candidates.iter().enumerate() {
        total_allocated = total_allocated.saturating_add(candidate.size.allocated_bytes);
        total_logical = total_logical.saturating_add(candidate.size.logical_bytes);

        let tier_cell =
            Cell::new(candidate.risk_tier.display_name()).fg(risk_tier_color(candidate.risk_tier));

        table.add_row(vec![
            Cell::new((idx + 1).to_string()),
            tier_cell,
            Cell::new(&candidate.scanner_name),
            Cell::new(candidate.size.human_readable_allocated()),
            Cell::new(candidate.size.human_readable_logical()),
            Cell::new(candidate.path.display().to_string()),
        ]);
    }

    println!("{table}");

    // Summary block
    println!("\n{}", "Summary:".bold());
    println!(
        "  Total Allocated Estimate: {}",
        AllocatedSize::format_human_bytes(total_allocated)
            .green()
            .bold()
    );
    println!(
        "  Total Logical Size:       {}",
        AllocatedSize::format_human_bytes(total_logical).cyan()
    );
    println!(
        "  Coverage Status:          {}",
        match coverage_status {
            CoverageStatus::Complete => "Complete (100% confidence)".green(),
            CoverageStatus::Degraded => "Degraded (Permission boundaries encountered)".yellow(),
            CoverageStatus::Incomplete => "Incomplete (Scan aborted or failed)".red(),
        }
    );

    print_diagnostics_summary(diagnostics);
}

/// Formats and prints an immutable execution Plan.
pub fn print_plan_table(plan: &Plan) {
    println!(
        "\n{}",
        format!("Execution Plan: {}", plan.metadata.plan_id)
            .bold()
            .underline()
    );
    println!("  Plan Hash:       {}", plan.metadata.plan_hash.dimmed());
    println!(
        "  Created At:      {}",
        plan.metadata.created_at.to_rfc2822().dimmed()
    );
    println!(
        "  Engine Version:  {}",
        plan.metadata.engine_version.dimmed()
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Approved").add_attribute(Attribute::Bold),
            Cell::new("Risk Tier").add_attribute(Attribute::Bold),
            Cell::new("Strategy").add_attribute(Attribute::Bold),
            Cell::new("Allocated").add_attribute(Attribute::Bold),
            Cell::new("Logical").add_attribute(Attribute::Bold),
            Cell::new("Path / Command").add_attribute(Attribute::Bold),
        ]);

    for action in &plan.actions {
        let approved_cell = if action.approved {
            Cell::new("[✓] YES").fg(Color::Green)
        } else {
            Cell::new("[ ] NO").fg(Color::DarkGrey)
        };

        let tier_cell =
            Cell::new(action.risk_tier.display_name()).fg(risk_tier_color(action.risk_tier));

        let strategy_str = match action.execution_strategy {
            mac_cleaner_core::model::candidate::ExecutionStrategy::NativeTrash => "Native Trash",
            mac_cleaner_core::model::candidate::ExecutionStrategy::ExternalCommand => {
                "External Command"
            }
            mac_cleaner_core::model::candidate::ExecutionStrategy::InPlacePurge => "In-Place Purge",
        };

        let target_str = if let Some(cmd) = &action.command {
            format!("Command: {}", cmd.join(" "))
        } else {
            action.path.display().to_string()
        };

        table.add_row(vec![
            approved_cell,
            tier_cell,
            Cell::new(strategy_str),
            Cell::new(action.expected_size.human_readable_allocated()),
            Cell::new(action.expected_size.human_readable_logical()),
            Cell::new(target_str),
        ]);
    }

    println!("{table}");

    let approved_size = plan.total_approved_size();
    let approved_count = plan.approved_actions().count();

    println!("\n{}", "Plan Totals:".bold());
    println!("  Total Candidates:         {}", plan.actions.len());
    println!(
        "  Approved Actions:         {}",
        approved_count.to_string().green().bold()
    );
    println!(
        "  Approved Space To Reclaim: {}",
        approved_size.human_readable_allocated().green().bold()
    );

    print_diagnostics_summary(&plan.diagnostics);
}

/// Formats and prints an Apply execution summary.
pub fn print_apply_summary(summary: &ApplySummary, journal_path: &Path) {
    println!("\n{}", "Reclamation Execution Summary".bold().underline());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("Metric").add_attribute(Attribute::Bold),
        Cell::new("Count / Value").add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec![
        Cell::new("Total Actions Processed"),
        Cell::new(summary.total_processed.to_string()),
    ]);
    table.add_row(vec![
        Cell::new("Successful Trash Relocations"),
        Cell::new(summary.successful_trash_moves.to_string()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("Successful External Commands"),
        Cell::new(summary.successful_external_commands.to_string()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("Successful In-Place Purges (irreversible)"),
        Cell::new(summary.successful_in_place_purges.to_string()).fg(Color::Yellow),
    ]);
    table.add_row(vec![
        Cell::new("Skipped Due to Drift"),
        Cell::new(summary.skipped_drifts.to_string()).fg(if summary.skipped_drifts > 0 {
            Color::Yellow
        } else {
            Color::White
        }),
    ]);
    table.add_row(vec![
        Cell::new("Failures"),
        Cell::new(summary.failures.to_string()).fg(if summary.failures > 0 {
            Color::Red
        } else {
            Color::Green
        }),
    ]);
    table.add_row(vec![
        Cell::new("Allocated Estimate Processed (not actual freed space)"),
        Cell::new(summary.reclaimed_size.human_readable_allocated()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("Observed Net Available-Space Change"),
        Cell::new(format_optional_space_delta(
            summary.observed_free_space_delta_bytes,
        )),
    ]);

    println!("{table}");

    if !summary.item_results.is_empty() {
        println!("\n{}", "Per-item outcomes:".bold());
        let mut items = Table::new();
        items.load_preset(UTF8_FULL);
        items.set_content_arrangement(ContentArrangement::Dynamic);
        items.set_header(vec![
            Cell::new("Result").add_attribute(Attribute::Bold),
            Cell::new("Allocated Estimate").add_attribute(Attribute::Bold),
            Cell::new("Path").add_attribute(Attribute::Bold),
            Cell::new("Details").add_attribute(Attribute::Bold),
        ]);

        for result in &summary.item_results {
            let color = match result.status {
                ApplyItemStatus::MovedToTrash | ApplyItemStatus::ExternalCommandSucceeded => {
                    Color::Green
                }
                ApplyItemStatus::PermanentlyPurged => Color::Yellow,
                ApplyItemStatus::SkippedDrift => Color::Yellow,
                ApplyItemStatus::Failed => Color::Red,
            };
            items.add_row(vec![
                Cell::new(result.status.display_name()).fg(color),
                Cell::new(result.allocated_estimate.human_readable_allocated()),
                Cell::new(result.path.display().to_string()),
                Cell::new(&result.details),
            ]);
        }
        println!("{items}");
    }

    if summary.successful_trash_moves > 0 {
        println!(
            "\nItems moved to native Trash remain recoverable with `mac-cleaner restore` and do not free disk space until you empty Trash. Empty Trash in Finder to release that space."
        );
    }
    println!(
        "Observed available-space change is volume-wide and may include unrelated activity; APFS snapshots and shared extents can also affect reclamation."
    );
    println!(
        "\nAudit Journal persisted at: {}",
        journal_path.display().to_string().cyan()
    );
}

fn format_optional_space_delta(delta_bytes: Option<i64>) -> String {
    match delta_bytes {
        Some(delta) => {
            let sign = if delta >= 0 { "+" } else { "-" };
            format!(
                "{sign}{}",
                AllocatedSize::format_human_bytes(delta.unsigned_abs())
            )
        }
        None => "Unavailable".to_string(),
    }
}

/// Formats and prints a Restore summary.
pub fn print_restore_summary(summary: &RestoreSummary) {
    println!("\n{}", "Restoration Summary".bold().underline());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("Metric").add_attribute(Attribute::Bold),
        Cell::new("Count").add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec![
        Cell::new("Total Items Attempted"),
        Cell::new(summary.total_attempted.to_string()),
    ]);
    table.add_row(vec![
        Cell::new("Successfully Restored"),
        Cell::new(summary.successfully_restored.to_string()).fg(Color::Green),
    ]);
    table.add_row(vec![
        Cell::new("Skipped (Destination Exists)"),
        Cell::new(summary.skipped_collisions.to_string()).fg(if summary.skipped_collisions > 0 {
            Color::Yellow
        } else {
            Color::White
        }),
    ]);
    table.add_row(vec![
        Cell::new("Not Found in Trash"),
        Cell::new(summary.not_found_in_trash.to_string()).fg(if summary.not_found_in_trash > 0 {
            Color::Red
        } else {
            Color::White
        }),
    ]);
    table.add_row(vec![
        Cell::new("Errors / Failures"),
        Cell::new(summary.failed.to_string()).fg(if summary.failed > 0 {
            Color::Red
        } else {
            Color::Green
        }),
    ]);

    println!("{table}");

    if !summary.item_results.is_empty() {
        println!("\n{}", "Per-item restoration outcomes:".bold());
        let mut items = Table::new();
        items.load_preset(UTF8_FULL);
        items.set_content_arrangement(ContentArrangement::Dynamic);
        items.set_header(vec![
            Cell::new("Result").add_attribute(Attribute::Bold),
            Cell::new("Original Path").add_attribute(Attribute::Bold),
            Cell::new("Trash Path").add_attribute(Attribute::Bold),
            Cell::new("Details").add_attribute(Attribute::Bold),
        ]);

        for result in &summary.item_results {
            let color = match result.status {
                RestoreItemStatus::Restored => Color::Green,
                RestoreItemStatus::DestinationCollision => Color::Yellow,
                RestoreItemStatus::NotFoundInTrash | RestoreItemStatus::Failed => Color::Red,
            };
            items.add_row(vec![
                Cell::new(result.status.display_name()).fg(color),
                Cell::new(result.original_path.display().to_string()),
                Cell::new(result.trash_path.display().to_string()),
                Cell::new(&result.details),
            ]);
        }
        println!("{items}");
    }
}

/// Represents summarized run history row.
#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub run_id: String,
    pub timestamp: String,
    pub actions_count: usize,
    pub trashed_count: usize,
    pub external_count: usize,
    pub purged_count: usize,
    pub reclaimed_allocated: String,
    pub status: String,
}

/// Formats and prints historical execution runs.
pub fn print_history_table(rows: &[HistoryRow]) {
    if rows.is_empty() {
        println!("{}", "No prior execution runs found in history.".dimmed());
        return;
    }

    println!("\n{}", "Run Execution History".bold().underline());

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Run / Manifest ID").add_attribute(Attribute::Bold),
            Cell::new("Timestamp").add_attribute(Attribute::Bold),
            Cell::new("Actions").add_attribute(Attribute::Bold),
            Cell::new("Trashed").add_attribute(Attribute::Bold),
            Cell::new("External").add_attribute(Attribute::Bold),
            Cell::new("Purged").add_attribute(Attribute::Bold),
            Cell::new("Allocated Estimate").add_attribute(Attribute::Bold),
            Cell::new("Recoverability").add_attribute(Attribute::Bold),
        ]);

    for row in rows {
        let status_cell = if row.status.contains("Recoverable") {
            Cell::new(&row.status).fg(Color::Green)
        } else if row.status.contains("Irreversible") {
            Cell::new(&row.status).fg(Color::Yellow)
        } else {
            Cell::new(&row.status).fg(Color::White)
        };

        table.add_row(vec![
            Cell::new(&row.run_id).fg(Color::Cyan),
            Cell::new(&row.timestamp),
            Cell::new(row.actions_count.to_string()),
            Cell::new(row.trashed_count.to_string()),
            Cell::new(row.external_count.to_string()),
            Cell::new(row.purged_count.to_string()),
            Cell::new(&row.reclaimed_allocated).fg(Color::Green),
            status_cell,
        ]);
    }

    println!("{table}");
}

/// Formats and prints non-fatal diagnostics collected during scan.
pub fn print_diagnostics_summary(diagnostics: &[ScanDiagnostic]) {
    if diagnostics.is_empty() {
        return;
    }

    println!(
        "\n{}",
        format!("Diagnostics & Non-Fatal Notices ({}):", diagnostics.len())
            .yellow()
            .bold()
    );

    for (i, diag) in diagnostics.iter().enumerate().take(15) {
        let path_str = diag
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  [{}] ({}) {}: {}",
            i + 1,
            diag.scanner_name.cyan(),
            path_str.dimmed(),
            diag.message
        );
    }

    if diagnostics.len() > 15 {
        println!(
            "  ... and {} more non-fatal notices.",
            diagnostics.len() - 15
        );
    }
}
