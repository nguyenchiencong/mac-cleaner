use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Safety classification assigned to a candidate defining its reversibility and operational impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    #[serde(rename = "tier1_redownloadable")]
    Tier1ReDownloadable,
    #[serde(rename = "tier2_rebuildable")]
    Tier2Rebuildable,
    #[serde(rename = "tier3_retained")]
    Tier3Retained,
    #[serde(rename = "tier4_user_data")]
    Tier4UserData,
}

impl RiskTier {
    /// Human-friendly display label for the risk tier.
    pub fn display_name(&self) -> &'static str {
        match self {
            RiskTier::Tier1ReDownloadable => "Tier 1: Re-downloadable",
            RiskTier::Tier2Rebuildable => "Tier 2: Rebuildable",
            RiskTier::Tier3Retained => "Tier 3: Retained",
            RiskTier::Tier4UserData => "Tier 4: User Data",
        }
    }

    /// Detailed description of the operational impact and recoverability.
    pub fn description(&self) -> &'static str {
        match self {
            RiskTier::Tier1ReDownloadable => {
                "Remote caches, package manager downloads, derived data that can be re-fetched automatically"
            }
            RiskTier::Tier2Rebuildable => {
                "Project build artifacts and dependencies (target/, node_modules/) that can be recompiled"
            }
            RiskTier::Tier3Retained => {
                "Retained artifacts, logs, simulator states, and crash reports that may contain diagnostic context"
            }
            RiskTier::Tier4UserData => {
                "User source code, configurations, and uncommitted databases (never auto-cleaned)"
            }
        }
    }

    /// Whether this tier is considered safe for automated selection in plans.
    pub fn is_safe_for_auto_clean(&self) -> bool {
        matches!(self, RiskTier::Tier1ReDownloadable | RiskTier::Tier2Rebuildable)
    }
}

/// The specific mechanism used to process an approved action on a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStrategy {
    /// Default execution mode: move to native macOS Trash (reversible).
    NativeTrash,
    /// Irreversible tool delegation (e.g. `xcrun simctl erase`, `brew cleanup`).
    ExternalCommand,
    /// Irreversible direct in-place purge.
    InPlacePurge,
}

impl ExecutionStrategy {
    /// Indicates if this strategy is reversible via native Trash restoration.
    pub fn is_reversible(&self) -> bool {
        matches!(self, ExecutionStrategy::NativeTrash)
    }
}

/// Physical and logical storage estimation for a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AllocatedSize {
    /// Logical file size in bytes (sum of `st_size`).
    pub logical_bytes: u64,
    /// Allocated physical disk space in bytes (sum of APFS `st_blocks * 512`).
    pub allocated_bytes: u64,
}

impl AllocatedSize {
    pub const ZERO: Self = Self {
        logical_bytes: 0,
        allocated_bytes: 0,
    };

    pub fn new(logical_bytes: u64, allocated_bytes: u64) -> Self {
        Self {
            logical_bytes,
            allocated_bytes,
        }
    }

    pub fn zero() -> Self {
        Self::ZERO
    }

    pub fn from_bytes(logical_bytes: u64, allocated_bytes: u64) -> Self {
        Self {
            logical_bytes,
            allocated_bytes,
        }
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            logical_bytes: self.logical_bytes.saturating_add(other.logical_bytes),
            allocated_bytes: self.allocated_bytes.saturating_add(other.allocated_bytes),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.logical_bytes == 0 && self.allocated_bytes == 0
    }

    pub fn format_human_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn human_readable_allocated(&self) -> String {
        Self::format_human_bytes(self.allocated_bytes)
    }

    pub fn human_readable_logical(&self) -> String {
        Self::format_human_bytes(self.logical_bytes)
    }
}

impl std::ops::Add for AllocatedSize {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        self.saturating_add(rhs)
    }
}

impl std::ops::AddAssign for AllocatedSize {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.saturating_add(rhs);
    }
}

/// Filesystem metadata captured to verify candidate identity immediately before execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevalidationFingerprint {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_sec: i64,
    pub mtime_nsec: i64,
    pub nlink: u64,
    pub is_dir: bool,
}

impl RevalidationFingerprint {
    /// Captures a fingerprint from a path using `symlink_metadata` (never following symlinks).
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let meta = std::fs::symlink_metadata(path.as_ref())?;
        Ok(Self::from_metadata(&meta))
    }

    /// Constructs a fingerprint from existing `Metadata`.
    pub fn from_metadata(meta: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                dev: meta.dev(),
                ino: meta.ino(),
                size: meta.size(),
                mtime_sec: meta.mtime(),
                mtime_nsec: meta.mtime_nsec(),
                nlink: meta.nlink(),
                is_dir: meta.is_dir(),
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                dev: 0,
                ino: 0,
                size: meta.len(),
                mtime_sec: 0,
                mtime_nsec: 0,
                nlink: 1,
                is_dir: meta.is_dir(),
            }
        }
    }

    /// Verifies if two fingerprints match exactly.
    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }

    /// Pre-flight revalidation check against a live path.
    pub fn revalidate<P: AsRef<Path>>(&self, path: P) -> Result<bool, std::io::Error> {
        match Self::from_path(path) {
            Ok(current) => Ok(self.matches(&current)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// A specific filesystem path or external resource discovered during a scan eligible for assessment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: Uuid,
    pub path: PathBuf,
    pub scanner_name: String,
    pub risk_tier: RiskTier,
    pub execution_strategy: ExecutionStrategy,
    pub size: AllocatedSize,
    pub fingerprint: RevalidationFingerprint,
    pub description: String,
    pub discovered_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl Candidate {
    pub fn new(
        path: PathBuf,
        scanner_name: impl Into<String>,
        risk_tier: RiskTier,
        execution_strategy: ExecutionStrategy,
        size: AllocatedSize,
        fingerprint: RevalidationFingerprint,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            path,
            scanner_name: scanner_name.into(),
            risk_tier,
            execution_strategy,
            size,
            fingerprint,
            description: description.into(),
            discovered_at: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_risk_tier_safety_rules() {
        assert!(RiskTier::Tier1ReDownloadable.is_safe_for_auto_clean());
        assert!(RiskTier::Tier2Rebuildable.is_safe_for_auto_clean());
        assert!(!RiskTier::Tier3Retained.is_safe_for_auto_clean());
        assert!(!RiskTier::Tier4UserData.is_safe_for_auto_clean());
    }

    #[test]
    fn test_execution_strategy_reversibility() {
        assert!(ExecutionStrategy::NativeTrash.is_reversible());
        assert!(!ExecutionStrategy::ExternalCommand.is_reversible());
        assert!(!ExecutionStrategy::InPlacePurge.is_reversible());
    }

    #[test]
    fn test_allocated_size_arithmetic_and_formatting() {
        let size1 = AllocatedSize::new(1024, 4096);
        let size2 = AllocatedSize::new(2048, 8192);
        let combined = size1 + size2;

        assert_eq!(combined.logical_bytes, 3072);
        assert_eq!(combined.allocated_bytes, 12288);
        assert_eq!(AllocatedSize::format_human_bytes(500), "500 B");
        assert_eq!(AllocatedSize::format_human_bytes(2048), "2.00 KB");
        assert_eq!(AllocatedSize::format_human_bytes(10 * 1024 * 1024), "10.00 MB");
        assert_eq!(AllocatedSize::format_human_bytes(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[test]
    fn test_revalidation_fingerprint() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), b"hello world").unwrap();

        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        assert_eq!(fp.size, 11);
        assert!(!fp.is_dir);

        // Revalidate matches
        assert!(fp.revalidate(temp_file.path()).unwrap());

        // Modify file -> size changes -> fingerprint mismatch
        std::fs::write(temp_file.path(), b"hello world modified").unwrap();
        assert!(!fp.revalidate(temp_file.path()).unwrap());
    }

    #[test]
    fn test_candidate_serialization() {
        let temp_file = NamedTempFile::new().unwrap();
        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        let candidate = Candidate::new(
            temp_file.path().to_path_buf(),
            "rust_target",
            RiskTier::Tier2Rebuildable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(100, 512),
            fp,
            "Target folder",
        ).with_metadata("project", "mac-cleaner");

        let json = serde_json::to_string(&candidate).unwrap();
        let deserialized: Candidate = serde_json::from_str(&json).unwrap();
        assert_eq!(candidate.id, deserialized.id);
        assert_eq!(candidate.path, deserialized.path);
        assert_eq!(candidate.risk_tier, deserialized.risk_tier);
        assert_eq!(candidate.metadata.get("project").unwrap(), "mac-cleaner");
    }
}
