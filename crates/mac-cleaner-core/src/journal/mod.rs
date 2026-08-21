use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use chrono::Utc;
use uuid::Uuid;

use crate::model::manifest::{JournalEntryKind, RunJournalEntry, TrashReceipt};

/// Append-only journal writer for audit records.
#[derive(Debug)]
pub struct JournalWriter {
    path: PathBuf,
    file: File,
}

impl JournalWriter {
    /// Creates or opens an append-only journal file.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        Ok(Self { path, file })
    }

    /// Creates a standard timestamped journal file in the default or specified directory.
    pub fn create_default_in(dir: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let id = Uuid::new_v4();
        let filename = format!("run_{}_{}.jsonl", timestamp, id);
        let path = dir.as_ref().join(filename);
        Self::new(path)
    }

    /// Appends a journal entry as a single newline-delimited JSON record.
    pub fn append(&mut self, entry: &RunJournalEntry) -> Result<(), std::io::Error> {
        let json_line = serde_json::to_string(entry)?;
        writeln!(self.file, "{}", json_line)?;
        self.file.flush()?;
        Ok(())
    }

    /// Returns the path to the journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Reader for parsing append-only run journals.
pub struct JournalReader;

impl JournalReader {
    /// Reads all journal entries from a file.
    pub fn read_entries(path: impl AsRef<Path>) -> Result<Vec<RunJournalEntry>, std::io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: RunJournalEntry = serde_json::from_str(trimmed)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Extracts all successful trash receipts from a journal file for restoration.
    pub fn read_trash_receipts(path: impl AsRef<Path>) -> Result<Vec<TrashReceipt>, std::io::Error> {
        let entries = Self::read_entries(path)?;
        let receipts = entries
            .into_iter()
            .filter_map(|e| match e.kind {
                JournalEntryKind::TrashSuccess => e.receipt,
                _ => None,
            })
            .collect();
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::model::candidate::{AllocatedSize, RevalidationFingerprint};

    #[test]
    fn test_journal_append_and_read() {
        let dir = tempdir().unwrap();
        let mut writer = JournalWriter::create_default_in(dir.path()).unwrap();

        let plan_id = Uuid::new_v4();
        let action_id = Uuid::new_v4();
        let path = PathBuf::from("/test/path");

        let entry1 = RunJournalEntry::intent(plan_id, action_id, path.clone());
        let receipt = TrashReceipt::new(
            path.clone(),
            PathBuf::from("/trash/path"),
            RevalidationFingerprint {
                dev: 1,
                ino: 2,
                size: 3,
                mtime_sec: 4,
                mtime_nsec: 5,
                nlink: 1,
                is_dir: false,
            },
            AllocatedSize::new(100, 512),
        );
        let entry2 = RunJournalEntry::trash_success(plan_id, action_id, path.clone(), receipt.clone());

        writer.append(&entry1).unwrap();
        writer.append(&entry2).unwrap();

        let entries = JournalReader::read_entries(writer.path()).unwrap();
        assert_eq!(entries.len(), 2);

        let receipts = JournalReader::read_trash_receipts(writer.path()).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0], receipt);
    }
}
