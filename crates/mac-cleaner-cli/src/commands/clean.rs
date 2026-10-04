use anyhow::Result;
use clap::Args;
use colored::Colorize;
use console::{measure_text_width, truncate_str, Term};
use dialoguer::{Confirm, MultiSelect};
use is_terminal::IsTerminal;
use std::path::{Path, PathBuf};
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

use crate::commands::progress::ScanProgress;
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
    println!(
        "{}",
        "🔍 Scanning system and configured workspaces...".bold()
    );

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

    let scan_progress = ScanProgress::start("Scanning configured workspaces and caches...");
    let scan_results = if let Some(cat) = &args.category {
        registry.execute_filtered(&ctx, |scanner| matches_category(scanner.name(), cat))
    } else {
        registry.execute_all(&ctx)
    };
    drop(scan_progress);

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
                diagnostics.push(
                    mac_cleaner_core::model::diagnostic::ScanDiagnostic::io_error(
                        None,
                        "scanner",
                        e.to_string(),
                    ),
                );
                merged_coverage = CoverageStatus::Incomplete;
            }
        }
    }

    candidates.sort_by(|left, right| {
        right
            .size
            .allocated_bytes
            .cmp(&left.size.allocated_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });

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

    let is_interactive = std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
        && !args.yes;

    let selected_indices: Vec<usize> = if is_interactive {
        let total_allocated = candidates.iter().fold(0u64, |total, candidate| {
            total.saturating_add(candidate.size.allocated_bytes)
        });
        let defaults: Vec<bool> = candidates
            .iter()
            .map(|candidate| is_auto_clean_candidate(candidate, merged_coverage))
            .collect();
        let initial_selected = candidates
            .iter()
            .zip(&defaults)
            .filter(|(_, selected)| **selected)
            .count();
        let term = Term::stderr();
        let (rows, columns) = term.size();
        let terminal_width = if columns == 0 { 80 } else { columns as usize };
        let max_length = usize::from(rows.saturating_sub(8)).max(1);

        println!(
            "\n{}",
            format!(
                "Found {} candidates · {} allocated estimate · {} selected by default",
                candidates.len(),
                mac_cleaner_core::model::candidate::AllocatedSize::format_human_bytes(
                    total_allocated
                ),
                initial_selected
            )
            .bold()
        );
        println!("↑/↓ move · Space toggle · a toggle all · Enter confirm · Esc/q cancel");

        let home = std::env::var_os("HOME").map(PathBuf::from);
        let items: Vec<String> = candidates
            .iter()
            .map(|candidate| format_candidate_line(candidate, terminal_width, home.as_deref()))
            .collect();

        match MultiSelect::new()
            .items(&items)
            .defaults(&defaults)
            .max_length(max_length)
            .interact_opt()?
        {
            Some(indices) => indices,
            None => {
                println!("{}", "Reclamation cancelled by user.".yellow());
                return Ok(());
            }
        }
    } else {
        // Non-interactive or --yes: select all safe candidates
        auto_clean_indices(&candidates, merged_coverage)
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
    let trash_count = plan
        .approved_actions()
        .filter(|action| {
            action.execution_strategy
                == mac_cleaner_core::model::candidate::ExecutionStrategy::NativeTrash
        })
        .count();

    println!(
        "\n{}",
        format!(
            "Selected {} item(s), with an allocated estimate of {} (not guaranteed freed space).",
            approved_count,
            approved_size.human_readable_allocated().green().bold(),
        )
        .bold()
    );
    if trash_count > 0 {
        println!(
            "Native Trash moves remain recoverable and free space only after Trash is emptied."
        );
    }

    if is_interactive {
        let proceed = Confirm::new()
            .with_prompt("Do you want to proceed with reclamation now?")
            .default(true)
            .interact_opt()?;

        if proceed != Some(true) {
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
    if summary.successful_trash_moves > 0 {
        println!(
            "To restore items still in Trash, run:\n  {}",
            format!("mac-cleaner restore {}", run_id).cyan().bold()
        );
    } else {
        println!(
            "No items from this run were moved to Trash; irreversible actions cannot be restored."
        );
    }

    Ok(())
}

fn is_auto_clean_candidate(candidate: &Candidate, coverage: CoverageStatus) -> bool {
    if !coverage.is_complete() {
        return false;
    }

    candidate
        .metadata
        .get("safe_for_auto_clean")
        .map(|value| value == "true")
        .unwrap_or_else(|| candidate.risk_tier.is_safe_for_auto_clean())
}

fn auto_clean_indices(candidates: &[Candidate], coverage: CoverageStatus) -> Vec<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            is_auto_clean_candidate(candidate, coverage).then_some(index)
        })
        .collect()
}

fn format_candidate_line(candidate: &Candidate, width: usize, home: Option<&Path>) -> String {
    let tier = match candidate.risk_tier {
        mac_cleaner_core::model::candidate::RiskTier::Tier1ReDownloadable => "Tier 1",
        mac_cleaner_core::model::candidate::RiskTier::Tier2Rebuildable => "Tier 2",
        mac_cleaner_core::model::candidate::RiskTier::Tier3Retained => "Tier 3",
        mac_cleaner_core::model::candidate::RiskTier::Tier4UserData => "Tier 4",
    };
    let target = if let Some(command) = candidate.metadata.get("command") {
        let command = serde_json::from_str::<Vec<String>>(command)
            .map(|parts| parts.join(" "))
            .unwrap_or_else(|_| command.clone());
        format!("Command: {}", sanitize_terminal_text(&command))
    } else {
        display_path(&candidate.path, home)
    };
    let size = candidate.size.human_readable_allocated();
    format_candidate_parts(tier, &size, &target, width)
}

fn display_path(path: &Path, home: Option<&Path>) -> String {
    let display = match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    };
    sanitize_terminal_text(&display)
}

fn sanitize_terminal_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn format_candidate_parts(tier: &str, size: &str, target: &str, width: usize) -> String {
    let prefix = format!("{tier:<6} {size:>10}  ");
    let width = width.max(1);
    let prefix_width = measure_text_width(&prefix);
    if prefix_width >= width {
        return truncate_str(&prefix, width, "").into_owned();
    }

    let target = middle_truncate(target, width - prefix_width);
    format!("{prefix}{target}")
}

fn middle_truncate(value: &str, width: usize) -> String {
    if measure_text_width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }

    let ellipsis = "…";
    if width == 1 {
        return ellipsis.to_string();
    }

    let chars: Vec<char> = value.chars().collect();
    let available = width - measure_text_width(ellipsis);
    let prefix_limit = available.div_ceil(2);
    let suffix_limit = available / 2;

    let mut prefix = String::new();
    let mut prefix_end = 0;
    for (index, character) in chars.iter().enumerate() {
        let mut candidate = prefix.clone();
        candidate.push(*character);
        if measure_text_width(&candidate) > prefix_limit {
            break;
        }
        prefix = candidate;
        prefix_end = index + 1;
    }

    let mut suffix_chars = Vec::new();
    for index in (prefix_end..chars.len()).rev() {
        let mut candidate = chars[index].to_string();
        candidate.extend(suffix_chars.iter());
        if measure_text_width(&candidate) > suffix_limit {
            break;
        }
        suffix_chars.insert(0, chars[index]);
    }

    let mut result = format!(
        "{prefix}{ellipsis}{}",
        suffix_chars.iter().collect::<String>()
    );
    while measure_text_width(&result) > width {
        if !suffix_chars.is_empty() {
            suffix_chars.remove(0);
        } else if !prefix.is_empty() {
            prefix.pop();
        } else {
            return ellipsis.to_string();
        }
        result = format!(
            "{prefix}{ellipsis}{}",
            suffix_chars.iter().collect::<String>()
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mac_cleaner_core::model::candidate::{
        AllocatedSize, ExecutionStrategy, RevalidationFingerprint, RiskTier,
    };
    use tempfile::tempdir;

    #[test]
    fn auto_clean_selection_only_includes_safe_candidates_with_complete_coverage() {
        let directory = tempdir().unwrap();
        let safe_path = directory.path().join("safe-cache");
        let retained_path = directory.path().join("retained-artifact");
        std::fs::write(&safe_path, b"cache").unwrap();
        std::fs::write(&retained_path, b"artifact").unwrap();

        let candidates = [
            Candidate::new(
                safe_path.clone(),
                "test",
                RiskTier::Tier1ReDownloadable,
                ExecutionStrategy::NativeTrash,
                AllocatedSize::new(5, 512),
                RevalidationFingerprint::from_path(&safe_path).unwrap(),
                "Cache",
            ),
            Candidate::new(
                retained_path.clone(),
                "test",
                RiskTier::Tier3Retained,
                ExecutionStrategy::NativeTrash,
                AllocatedSize::new(8, 512),
                RevalidationFingerprint::from_path(&retained_path).unwrap(),
                "Retained artifact",
            ),
        ];

        assert_eq!(auto_clean_indices(&candidates, CoverageStatus::Complete), vec![0]);
        assert!(auto_clean_indices(&candidates, CoverageStatus::Incomplete).is_empty());
    }

    #[test]
    fn candidate_line_fits_terminal_width_and_preserves_path_ends() {
        let line = format_candidate_parts(
            "Tier 2",
            "12.00 MB",
            "~/workspace/a-very-long-repository-name/target",
            38,
        );

        assert!(measure_text_width(&line) <= 38);
        assert!(line.starts_with("Tier 2"));
        assert!(line.contains("…"));
        assert!(line.ends_with("target"));
    }

    #[test]
    fn candidate_line_truncates_unicode_to_narrow_terminal() {
        let line = format_candidate_parts("Tier 1", "1.00 KB", "~/開発/非常に長い名前/cache", 25);
        assert!(measure_text_width(&line) <= 25);
        assert!(line.contains("…"));
    }

    #[test]
    fn middle_truncation_handles_zero_and_one_column() {
        assert_eq!(middle_truncate("long path", 0), "");
        assert_eq!(middle_truncate("long path", 1), "…");
    }
}
