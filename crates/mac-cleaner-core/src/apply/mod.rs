use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use crate::journal::JournalWriter;
use crate::model::candidate::{AllocatedSize, ExecutionStrategy, RevalidationFingerprint};
use crate::model::manifest::RunJournalEntry;
use crate::model::plan::Plan;
use crate::platform::process::{ProcessError, ProcessExecutor};
use crate::platform::trash::{TrashBackend, TrashError};

#[derive(Error, Debug)]
pub enum ApplyError {
    #[error("Plan hash verification failed: plan may have been tampered with")]
    PlanTampered,

    #[error("Journal I/O error: {0}")]
    JournalIo(#[from] std::io::Error),

    #[error("Trash operation failed: {0}")]
    Trash(#[from] TrashError),

    #[error("External command execution failed: {0}")]
    Process(#[from] ProcessError),
}

/// Summary report of an apply execution run.
#[derive(Debug, Clone, Default)]
pub struct ApplySummary {
    pub total_processed: usize,
    pub successful_trash_moves: usize,
    pub successful_external_commands: usize,
    pub skipped_drifts: usize,
    pub failures: usize,
    pub reclaimed_size: AllocatedSize,
}

/// Execution engine executing approved actions from plans with strict pre-flight revalidation.
pub struct ApplyEngine {
    trash_backend: Arc<dyn TrashBackend>,
    process_executor: Arc<ProcessExecutor>,
}

impl ApplyEngine {
    pub fn new(
        trash_backend: Arc<dyn TrashBackend>,
        process_executor: Arc<ProcessExecutor>,
    ) -> Self {
        Self {
            trash_backend,
            process_executor,
        }
    }

    /// Executes approved actions in the plan, writing all operations to the journal.
    pub fn apply(
        &self,
        plan: &Plan,
        journal: &mut JournalWriter,
    ) -> Result<ApplySummary, ApplyError> {
        if !plan.verify_hash() {
            return Err(ApplyError::PlanTampered);
        }

        let mut summary = ApplySummary::default();
        let plan_id = plan.metadata.plan_id;

        for action in plan.approved_actions() {
            summary.total_processed += 1;

            // 1. Write Intent to journal
            journal.append(&RunJournalEntry::intent(
                plan_id,
                action.action_id,
                action.path.clone(),
            ))?;

            // 2. Pre-flight Revalidation (ADR 0002)
            match RevalidationFingerprint::from_path(&action.path) {
                Ok(current_fp) => {
                    if !action.fingerprint.matches(&current_fp) {
                        let reason = format!(
                            "Pre-flight drift detected for {}: metadata changed between scan and apply",
                            action.path.display()
                        );
                        journal.append(&RunJournalEntry::skipped_drift(
                            plan_id,
                            action.action_id,
                            action.path.clone(),
                            &reason,
                        ))?;
                        summary.skipped_drifts += 1;
                        continue;
                    }
                }
                Err(e) => {
                    let reason = format!(
                        "Path vanished or inaccessible before execution: {}",
                        e
                    );
                    journal.append(&RunJournalEntry::skipped_drift(
                        plan_id,
                        action.action_id,
                        action.path.clone(),
                        reason,
                    ))?;
                    summary.skipped_drifts += 1;
                    continue;
                }
            }

            // 3. Execution based on strategy
            match action.execution_strategy {
                ExecutionStrategy::NativeTrash => {
                    match self.trash_backend.move_to_trash(&action.path) {
                        Ok(receipt) => {
                            summary.successful_trash_moves += 1;
                            summary.reclaimed_size += receipt.size;
                            journal.append(&RunJournalEntry::trash_success(
                                plan_id,
                                action.action_id,
                                action.path.clone(),
                                receipt,
                            ))?;
                        }
                        Err(e) => {
                            summary.failures += 1;
                            journal.append(&RunJournalEntry::failed(
                                plan_id,
                                action.action_id,
                                action.path.clone(),
                                format!("Trash move failed: {}", e),
                            ))?;
                        }
                    }
                }
                ExecutionStrategy::ExternalCommand => {
                    if let Some(cmd) = &action.command {
                        if cmd.is_empty() {
                            summary.failures += 1;
                            journal.append(&RunJournalEntry::failed(
                                plan_id,
                                action.action_id,
                                action.path.clone(),
                                "External command was empty",
                            ))?;
                            continue;
                        }

                        let program = &cmd[0];
                        let args: Vec<&str> = cmd[1..].iter().map(|s| s.as_str()).collect();

                        match self.process_executor.execute(
                            program,
                            &args,
                            Some(Duration::from_secs(300)),
                        ) {
                            Ok(output) => {
                                if output.success {
                                    summary.successful_external_commands += 1;
                                    summary.reclaimed_size += action.expected_size;
                                    journal.append(&RunJournalEntry::external_success(
                                        plan_id,
                                        action.action_id,
                                        action.path.clone(),
                                        output.exit_code,
                                        output.duration.as_millis() as u64,
                                        Some(output.stdout),
                                    ))?;
                                } else {
                                    summary.failures += 1;
                                    journal.append(&RunJournalEntry::failed(
                                        plan_id,
                                        action.action_id,
                                        action.path.clone(),
                                        format!(
                                            "Command exited with code {:?}: {}",
                                            output.exit_code, output.stderr
                                        ),
                                    ))?;
                                }
                            }
                            Err(e) => {
                                summary.failures += 1;
                                journal.append(&RunJournalEntry::failed(
                                    plan_id,
                                    action.action_id,
                                    action.path.clone(),
                                    format!("Process execution error: {}", e),
                                ))?;
                            }
                        }
                    }
                }
                ExecutionStrategy::InPlacePurge => {
                    let res = if action.fingerprint.is_dir {
                        std::fs::remove_dir_all(&action.path)
                    } else {
                        std::fs::remove_file(&action.path)
                    };

                    match res {
                        Ok(_) => {
                            summary.successful_trash_moves += 1;
                            summary.reclaimed_size += action.expected_size;
                            journal.append(&RunJournalEntry::trash_success(
                                plan_id,
                                action.action_id,
                                action.path.clone(),
                                crate::model::manifest::TrashReceipt::new(
                                    action.path.clone(),
                                    action.path.clone(),
                                    action.fingerprint,
                                    action.expected_size,
                                ),
                            ))?;
                        }
                        Err(e) => {
                            summary.failures += 1;
                            journal.append(&RunJournalEntry::failed(
                                plan_id,
                                action.action_id,
                                action.path.clone(),
                                format!("In-place purge failed: {}", e),
                            ))?;
                        }
                    }
                }
            }
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::model::candidate::{Candidate, RiskTier};
    use crate::model::diagnostic::CoverageStatus;
    use crate::platform::trash::FakeTrashBackend;

    #[test]
    fn test_apply_and_revalidation_lifecycle() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let journal_dir = tempdir().unwrap();

        let trash_backend = Arc::new(FakeTrashBackend::new(trash_dir.path()));
        let process_executor = Arc::new(ProcessExecutor::new());
        let engine = ApplyEngine::new(trash_backend.clone(), process_executor);

        // 1. Setup two target files
        let file1 = work_dir.path().join("clean_me.tmp");
        let file2 = work_dir.path().join("drift_me.tmp");
        std::fs::write(&file1, b"cleanable content").unwrap();
        std::fs::write(&file2, b"initial content").unwrap();

        let fp1 = RevalidationFingerprint::from_path(&file1).unwrap();
        let fp2 = RevalidationFingerprint::from_path(&file2).unwrap();

        let c1 = Candidate::new(
            file1.clone(),
            "temp_cleaner",
            RiskTier::Tier1ReDownloadable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(17, 512),
            fp1,
            "Cleanable file",
        );
        let c2 = Candidate::new(
            file2.clone(),
            "temp_cleaner",
            RiskTier::Tier1ReDownloadable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(15, 512),
            fp2,
            "Drifting file",
        );

        let plan = Plan::from_candidates(
            vec![c1, c2],
            CoverageStatus::Complete,
            vec![],
            true, // auto-approve
        );

        // Now drift file2 by writing new content before apply!
        std::fs::write(&file2, b"modified content by another process").unwrap();

        let mut journal = JournalWriter::create_default_in(journal_dir.path()).unwrap();
        let summary = engine.apply(&plan, &mut journal).unwrap();

        // Check summary
        assert_eq!(summary.total_processed, 2);
        assert_eq!(summary.successful_trash_moves, 1);
        assert_eq!(summary.skipped_drifts, 1);
        assert_eq!(summary.failures, 0);

        // file1 should be trashed
        assert!(!file1.exists());
        // file2 should NOT be touched because it drifted
        assert!(file2.exists());
        assert_eq!(std::fs::read(&file2).unwrap(), b"modified content by another process");
    }
}
