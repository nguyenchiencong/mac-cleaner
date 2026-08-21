pub mod candidate;
pub mod diagnostic;
pub mod manifest;
pub mod plan;

pub use candidate::{
    AllocatedSize, Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier,
};
pub use diagnostic::{CoverageStatus, DiagnosticKind, ScanDiagnostic};
pub use manifest::{JournalEntryKind, RunJournalEntry, TrashReceipt};
pub use plan::{Plan, PlanMetadata, PlannedAction, CURRENT_PLAN_SCHEMA_VERSION};
