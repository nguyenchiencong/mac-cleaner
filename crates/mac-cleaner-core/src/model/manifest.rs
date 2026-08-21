use std::path::PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::candidate::{AllocatedSize, RevalidationFingerprint};

/// A record returned upon successful relocation to Trash, capturing the original path,
/// destination identifier/path, timestamp, and size/fingerprint for auditing and restoration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashReceipt {
    pub receipt_id: Uuid,
    pub original_path: PathBuf,
    pub trash_path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub fingerprint: RevalidationFingerprint,
    pub size: AllocatedSize,
}

impl TrashReceipt {
    pub fn new(
        original_path: PathBuf,
        trash_path: PathBuf,
        fingerprint: RevalidationFingerprint,
        size: AllocatedSize,
    ) -> Self {
        Self {
            receipt_id: Uuid::new_v4(),
            original_path,
            trash_path,
            timestamp: Utc::now(),
            fingerprint,
            size,
        }
    }
}

/// The specific outcome or state of an operation in the append-only run journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalEntryKind {
    /// Action intent registered immediately before execution begins.
    Intent,
    /// Candidate safely relocated to native Trash, yielding a TrashReceipt.
    TrashSuccess,
    /// External command executed successfully.
    ExternalSuccess {
        exit_code: Option<i32>,
        duration_ms: u64,
    },
    /// Candidate skipped because pre-flight revalidation detected filesystem drift or vanishing.
    SkippedDrift {
        reason: String,
    },
    /// Operation failed with an error.
    Failed {
        error: String,
    },
}

/// An append-only structured record persisted during execution that tracks operation
/// intents, actual outcomes, and destination paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunJournalEntry {
    pub entry_id: Uuid,
    pub plan_id: Uuid,
    pub action_id: Uuid,
    pub path: PathBuf,
    pub timestamp: DateTime<Utc>,
    pub kind: JournalEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<TrashReceipt>,
}

impl RunJournalEntry {
    pub fn intent(plan_id: Uuid, action_id: Uuid, path: PathBuf) -> Self {
        Self {
            entry_id: Uuid::new_v4(),
            plan_id,
            action_id,
            path,
            timestamp: Utc::now(),
            kind: JournalEntryKind::Intent,
            details: None,
            receipt: None,
        }
    }

    pub fn trash_success(
        plan_id: Uuid,
        action_id: Uuid,
        path: PathBuf,
        receipt: TrashReceipt,
    ) -> Self {
        Self {
            entry_id: Uuid::new_v4(),
            plan_id,
            action_id,
            path,
            timestamp: Utc::now(),
            kind: JournalEntryKind::TrashSuccess,
            details: Some(format!("Relocated to trash at {}", receipt.trash_path.display())),
            receipt: Some(receipt),
        }
    }

    pub fn external_success(
        plan_id: Uuid,
        action_id: Uuid,
        path: PathBuf,
        exit_code: Option<i32>,
        duration_ms: u64,
        details: Option<String>,
    ) -> Self {
        Self {
            entry_id: Uuid::new_v4(),
            plan_id,
            action_id,
            path,
            timestamp: Utc::now(),
            kind: JournalEntryKind::ExternalSuccess {
                exit_code,
                duration_ms,
            },
            details,
            receipt: None,
        }
    }

    pub fn skipped_drift(
        plan_id: Uuid,
        action_id: Uuid,
        path: PathBuf,
        reason: impl Into<String>,
    ) -> Self {
        let reason_str = reason.into();
        Self {
            entry_id: Uuid::new_v4(),
            plan_id,
            action_id,
            path,
            timestamp: Utc::now(),
            kind: JournalEntryKind::SkippedDrift {
                reason: reason_str.clone(),
            },
            details: Some(reason_str),
            receipt: None,
        }
    }

    pub fn failed(
        plan_id: Uuid,
        action_id: Uuid,
        path: PathBuf,
        error: impl Into<String>,
    ) -> Self {
        let err_str = error.into();
        Self {
            entry_id: Uuid::new_v4(),
            plan_id,
            action_id,
            path,
            timestamp: Utc::now(),
            kind: JournalEntryKind::Failed {
                error: err_str.clone(),
            },
            details: Some(err_str),
            receipt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_entry_serialization() {
        let plan_id = Uuid::new_v4();
        let action_id = Uuid::new_v4();
        let path = PathBuf::from("/tmp/test_dir");

        let entry = RunJournalEntry::intent(plan_id, action_id, path.clone());
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: RunJournalEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }
}
