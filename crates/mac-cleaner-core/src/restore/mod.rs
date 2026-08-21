use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use crate::journal::JournalReader;
use crate::model::manifest::TrashReceipt;
use crate::platform::trash::{RestoreError, TrashBackend};

#[derive(Error, Debug)]
pub enum RestorationError {
    #[error("Failed to read journal file: {0}")]
    JournalRead(#[from] std::io::Error),

    #[error("Restore failed for path '{path}': {source}")]
    RestoreFailed {
        path: String,
        source: RestoreError,
    },
}

/// Summary report of a restoration operation.
#[derive(Debug, Clone, Default)]
pub struct RestoreSummary {
    pub total_attempted: usize,
    pub successfully_restored: usize,
    pub skipped_collisions: usize,
    pub not_found_in_trash: usize,
    pub failed: usize,
}

/// Engine for recovering files from Trash using structured receipts.
pub struct RestoreEngine {
    trash_backend: Arc<dyn TrashBackend>,
}

impl RestoreEngine {
    pub fn new(trash_backend: Arc<dyn TrashBackend>) -> Self {
        Self { trash_backend }
    }

    /// Restores all trashed items recorded in a journal file.
    pub fn restore_from_journal(
        &self,
        journal_path: impl AsRef<Path>,
    ) -> Result<RestoreSummary, RestorationError> {
        let receipts = JournalReader::read_trash_receipts(journal_path)?;
        Ok(self.restore_receipts(&receipts))
    }

    /// Restores a specific list of receipts.
    pub fn restore_receipts(&self, receipts: &[TrashReceipt]) -> RestoreSummary {
        let mut summary = RestoreSummary::default();

        for receipt in receipts {
            summary.total_attempted += 1;

            match self.trash_backend.restore(receipt) {
                Ok(()) => {
                    summary.successfully_restored += 1;
                }
                Err(RestoreError::DestinationCollision { .. }) => {
                    summary.skipped_collisions += 1;
                }
                Err(RestoreError::TrashItemNotFound(_)) => {
                    summary.not_found_in_trash += 1;
                }
                Err(_) => {
                    summary.failed += 1;
                }
            }
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::platform::trash::FakeTrashBackend;

    #[test]
    fn test_restore_engine_cycle() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let backend = Arc::new(FakeTrashBackend::new(trash_dir.path()));
        let restore_engine = RestoreEngine::new(backend.clone());

        let orig_file = work_dir.path().join("restore_me.txt");
        std::fs::write(&orig_file, b"restore test data").unwrap();

        let receipt = backend.move_to_trash(&orig_file).unwrap();
        assert!(!orig_file.exists());

        let summary = restore_engine.restore_receipts(&[receipt]);
        assert_eq!(summary.total_attempted, 1);
        assert_eq!(summary.successfully_restored, 1);
        assert_eq!(summary.skipped_collisions, 0);
        assert_eq!(summary.failed, 0);

        assert!(orig_file.exists());
        assert_eq!(std::fs::read(&orig_file).unwrap(), b"restore test data");
    }
}
