use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

use crate::model::candidate::{
    AllocatedSize, Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier,
};
use crate::model::diagnostic::{CoverageStatus, ScanDiagnostic};

pub const CURRENT_PLAN_SCHEMA_VERSION: u32 = 1;

/// Metadata associated with an immutable execution plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanMetadata {
    pub plan_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub schema_version: u32,
    pub plan_hash: String,
    pub engine_version: String,
    pub total_candidates: usize,
    pub total_allocated_bytes: u64,
    pub total_logical_bytes: u64,
}

/// A specific action to be performed on a validated candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedAction {
    pub action_id: Uuid,
    pub candidate_id: Uuid,
    pub path: PathBuf,
    pub risk_tier: RiskTier,
    pub execution_strategy: ExecutionStrategy,
    pub expected_size: AllocatedSize,
    pub fingerprint: RevalidationFingerprint,
    pub description: String,
    pub command: Option<Vec<String>>,
    pub approved: bool,
}

impl PlannedAction {
    pub fn from_candidate(candidate: &Candidate, approved: bool) -> Self {
        let command = if candidate.execution_strategy == ExecutionStrategy::ExternalCommand {
            candidate
                .metadata
                .get("command")
                .and_then(|c| serde_json::from_str::<Vec<String>>(c).ok())
        } else {
            None
        };

        Self {
            action_id: Uuid::new_v4(),
            candidate_id: candidate.id,
            path: candidate.path.clone(),
            risk_tier: candidate.risk_tier,
            execution_strategy: candidate.execution_strategy,
            expected_size: candidate.size,
            fingerprint: candidate.fingerprint,
            description: candidate.description.clone(),
            command,
            approved,
        }
    }

    pub fn from_external_candidate(
        candidate: &Candidate,
        command: Vec<String>,
        approved: bool,
    ) -> Self {
        Self {
            action_id: Uuid::new_v4(),
            candidate_id: candidate.id,
            path: candidate.path.clone(),
            risk_tier: candidate.risk_tier,
            execution_strategy: ExecutionStrategy::ExternalCommand,
            expected_size: candidate.size,
            fingerprint: candidate.fingerprint,
            description: candidate.description.clone(),
            command: Some(command),
            approved,
        }
    }
}

/// An immutable, serialized set of approved actions generated from scanned candidates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub metadata: PlanMetadata,
    pub actions: Vec<PlannedAction>,
    pub coverage_status: CoverageStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ScanDiagnostic>,
}

impl Plan {
    /// Computes a deterministic SHA-256 hash over the actions in the plan.
    pub fn compute_canonical_hash(actions: &[PlannedAction]) -> String {
        let mut hasher = Sha256::new();
        for action in actions {
            hasher.update(action.action_id.as_bytes());
            hasher.update(action.candidate_id.as_bytes());
            hasher.update(action.path.to_string_lossy().as_bytes());
            hasher.update(format!("{:?}", action.risk_tier).as_bytes());
            hasher.update(format!("{:?}", action.execution_strategy).as_bytes());
            hasher.update(action.expected_size.logical_bytes.to_be_bytes());
            hasher.update(action.expected_size.allocated_bytes.to_be_bytes());
            hasher.update(action.fingerprint.dev.to_be_bytes());
            hasher.update(action.fingerprint.ino.to_be_bytes());
            hasher.update(action.fingerprint.size.to_be_bytes());
            hasher.update(action.fingerprint.mtime_sec.to_be_bytes());
            hasher.update(action.fingerprint.mtime_nsec.to_be_bytes());
            hasher.update(action.fingerprint.nlink.to_be_bytes());
            hasher.update([if action.fingerprint.is_dir { 1 } else { 0 }]);
            hasher.update([if action.approved { 1 } else { 0 }]);
            if let Some(cmd) = &action.command {
                for c in cmd {
                    hasher.update(c.as_bytes());
                }
            }
        }
        hex::encode(hasher.finalize())
    }

    /// Creates a new Plan from planned actions, computing totals and canonical hash.
    pub fn new(
        actions: Vec<PlannedAction>,
        coverage_status: CoverageStatus,
        diagnostics: Vec<ScanDiagnostic>,
    ) -> Self {
        let plan_id = Uuid::new_v4();
        let plan_hash = Self::compute_canonical_hash(&actions);
        let total_candidates = actions.len();
        let mut total_allocated_bytes = 0u64;
        let mut total_logical_bytes = 0u64;

        for action in &actions {
            total_allocated_bytes =
                total_allocated_bytes.saturating_add(action.expected_size.allocated_bytes);
            total_logical_bytes =
                total_logical_bytes.saturating_add(action.expected_size.logical_bytes);
        }

        let metadata = PlanMetadata {
            plan_id,
            created_at: Utc::now(),
            schema_version: CURRENT_PLAN_SCHEMA_VERSION,
            plan_hash,
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            total_candidates,
            total_allocated_bytes,
            total_logical_bytes,
        };

        Self {
            metadata,
            actions,
            coverage_status,
            diagnostics,
        }
    }

    /// Creates a Plan from discovered candidates, optionally auto-approving safe risk tiers.
    pub fn from_candidates(
        candidates: Vec<Candidate>,
        coverage_status: CoverageStatus,
        diagnostics: Vec<ScanDiagnostic>,
        auto_approve_safe: bool,
    ) -> Self {
        let actions = candidates
            .into_iter()
            .map(|candidate| {
                // If coverage is degraded, ADR 0006 states candidates should not be auto-selected.
                let is_safe = candidate
                    .metadata
                    .get("safe_for_auto_clean")
                    .map(|v| v == "true")
                    .unwrap_or_else(|| candidate.risk_tier.is_safe_for_auto_clean());

                let approved = if !coverage_status.is_complete() {
                    false
                } else if auto_approve_safe {
                    is_safe
                } else {
                    false
                };
                PlannedAction::from_candidate(&candidate, approved)
            })
            .collect();

        Self::new(actions, coverage_status, diagnostics)
    }

    /// Verifies if the plan actions match the metadata hash (ensuring no tampering).
    pub fn verify_hash(&self) -> bool {
        let recomputed = Self::compute_canonical_hash(&self.actions);
        recomputed == self.metadata.plan_hash
    }

    /// Returns iterator over approved actions.
    pub fn approved_actions(&self) -> impl Iterator<Item = &PlannedAction> {
        self.actions.iter().filter(|a| a.approved)
    }

    /// Returns mutable iterator over approved actions.
    pub fn approved_actions_mut(&mut self) -> impl Iterator<Item = &mut PlannedAction> {
        self.actions.iter_mut().filter(|a| a.approved)
    }

    /// Recalculates metadata after in-memory modifications (e.g. approval toggles).
    pub fn update_metadata(&mut self) {
        self.metadata.plan_hash = Self::compute_canonical_hash(&self.actions);
        self.metadata.total_candidates = self.actions.len();
        let mut total_allocated_bytes = 0u64;
        let mut total_logical_bytes = 0u64;
        for action in &self.actions {
            total_allocated_bytes =
                total_allocated_bytes.saturating_add(action.expected_size.allocated_bytes);
            total_logical_bytes =
                total_logical_bytes.saturating_add(action.expected_size.logical_bytes);
        }
        self.metadata.total_allocated_bytes = total_allocated_bytes;
        self.metadata.total_logical_bytes = total_logical_bytes;
    }

    /// Total allocated and logical size of approved actions.
    pub fn total_approved_size(&self) -> AllocatedSize {
        let mut size = AllocatedSize::zero();
        for action in self.approved_actions() {
            size += action.expected_size;
        }
        size
    }

    /// Serializes plan to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes plan from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_plan_hashing_and_tamper_detection() {
        let temp_file = NamedTempFile::new().unwrap();
        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        let candidate = Candidate::new(
            temp_file.path().to_path_buf(),
            "rust_target",
            RiskTier::Tier2Rebuildable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(1024, 4096),
            fp,
            "Rust target directory",
        );

        let plan = Plan::from_candidates(
            vec![candidate],
            CoverageStatus::Complete,
            vec![],
            true, // auto-approve safe
        );

        assert!(plan.verify_hash());
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].approved);

        // Tamper with action size
        let mut tampered_plan = plan.clone();
        tampered_plan.actions[0].expected_size = AllocatedSize::new(9999, 9999);
        assert!(!tampered_plan.verify_hash());

        // Update metadata re-syncs hash
        tampered_plan.update_metadata();
        assert!(tampered_plan.verify_hash());
    }

    #[test]
    fn test_degraded_coverage_disables_auto_approval() {
        let temp_file = NamedTempFile::new().unwrap();
        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        let candidate = Candidate::new(
            temp_file.path().to_path_buf(),
            "rust_target",
            RiskTier::Tier1ReDownloadable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(100, 512),
            fp,
            "Cache item",
        );

        // Coverage is degraded: auto_approve_safe = true MUST NOT approve
        let plan = Plan::from_candidates(vec![candidate], CoverageStatus::Degraded, vec![], true);

        assert_eq!(plan.actions.len(), 1);
        assert!(
            !plan.actions[0].approved,
            "Degraded coverage must prevent auto-approval"
        );
    }

    #[test]
    fn test_incomplete_coverage_disables_auto_approval() {
        let temp_file = NamedTempFile::new().unwrap();
        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        let candidate = Candidate::new(
            temp_file.path().to_path_buf(),
            "rust_target",
            RiskTier::Tier1ReDownloadable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(100, 512),
            fp,
            "Cache item",
        );

        let plan = Plan::from_candidates(vec![candidate], CoverageStatus::Incomplete, vec![], true);

        assert!(!plan.actions[0].approved);
    }

    #[test]
    fn test_plan_json_roundtrip() {
        let temp_file = NamedTempFile::new().unwrap();
        let fp = RevalidationFingerprint::from_path(temp_file.path()).unwrap();
        let candidate = Candidate::new(
            temp_file.path().to_path_buf(),
            "node_modules",
            RiskTier::Tier2Rebuildable,
            ExecutionStrategy::NativeTrash,
            AllocatedSize::new(2048, 8192),
            fp,
            "Node modules",
        );

        let plan = Plan::from_candidates(vec![candidate], CoverageStatus::Complete, vec![], true);

        let json = plan.to_json().unwrap();
        let deserialized = Plan::from_json(&json).unwrap();
        assert_eq!(plan, deserialized);
        assert!(deserialized.verify_hash());
    }
}
