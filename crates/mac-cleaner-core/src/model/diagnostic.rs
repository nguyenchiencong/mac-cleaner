use std::path::PathBuf;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Specific category of diagnostic event or barrier encountered during traversal or inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// Directory or file could not be read due to OS permissions (TCC / file permissions).
    PermissionDenied,
    /// Item existed when initially listed but vanished before metadata could be read.
    PathVanished,
    /// A symlink whose target does not exist.
    BrokenSymlink,
    /// Traversal of a subtree could not be completed, degrading evidence confidence.
    IncompleteCoverage,
    /// General I/O or system error.
    IoError(String),
}

/// Indicator of whether inspection completed cleanly or encountered barriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    /// Inspected completely with 100% confidence.
    #[default]
    Complete,
    /// Partially inspected due to permission barriers or non-fatal errors; candidates in affected subtrees should not be auto-selected.
    Degraded,
    /// Traversal aborted or failed entirely.
    Incomplete,
}

impl CoverageStatus {
    pub fn is_complete(&self) -> bool {
        matches!(self, CoverageStatus::Complete)
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self, CoverageStatus::Degraded)
    }

    /// Combines two coverage statuses, taking the most restrictive.
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (CoverageStatus::Incomplete, _) | (_, CoverageStatus::Incomplete) => {
                CoverageStatus::Incomplete
            }
            (CoverageStatus::Degraded, _) | (_, CoverageStatus::Degraded) => {
                CoverageStatus::Degraded
            }
            (CoverageStatus::Complete, CoverageStatus::Complete) => CoverageStatus::Complete,
        }
    }
}

/// Structured non-fatal record capturing skipped paths, permission boundaries, or filesystem anomalies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanDiagnostic {
    pub path: Option<PathBuf>,
    pub scanner_name: String,
    pub kind: DiagnosticKind,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

impl ScanDiagnostic {
    pub fn new(
        path: Option<PathBuf>,
        scanner_name: impl Into<String>,
        kind: DiagnosticKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path,
            scanner_name: scanner_name.into(),
            kind,
            message: message.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn permission_denied(
        path: PathBuf,
        scanner_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            Some(path),
            scanner_name,
            DiagnosticKind::PermissionDenied,
            message,
        )
    }

    pub fn path_vanished(
        path: PathBuf,
        scanner_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            Some(path),
            scanner_name,
            DiagnosticKind::PathVanished,
            message,
        )
    }

    pub fn broken_symlink(
        path: PathBuf,
        scanner_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            Some(path),
            scanner_name,
            DiagnosticKind::BrokenSymlink,
            message,
        )
    }

    pub fn incomplete_coverage(
        path: Option<PathBuf>,
        scanner_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            path,
            scanner_name,
            DiagnosticKind::IncompleteCoverage,
            message,
        )
    }

    pub fn io_error(
        path: Option<PathBuf>,
        scanner_name: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        let msg = error.into();
        Self::new(path, scanner_name, DiagnosticKind::IoError(msg.clone()), msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coverage_status_merge() {
        assert_eq!(
            CoverageStatus::Complete.merge(CoverageStatus::Complete),
            CoverageStatus::Complete
        );
        assert_eq!(
            CoverageStatus::Complete.merge(CoverageStatus::Degraded),
            CoverageStatus::Degraded
        );
        assert_eq!(
            CoverageStatus::Degraded.merge(CoverageStatus::Incomplete),
            CoverageStatus::Incomplete
        );
    }

    #[test]
    fn test_scan_diagnostic_constructors() {
        let diag = ScanDiagnostic::permission_denied(
            PathBuf::from("/Library/Caches/secret"),
            "system_cache",
            "Permission denied",
        );
        assert_eq!(diag.kind, DiagnosticKind::PermissionDenied);
        assert_eq!(diag.scanner_name, "system_cache");
        assert!(diag.path.is_some());
    }
}
