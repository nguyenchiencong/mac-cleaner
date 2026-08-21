use crate::model::candidate::Candidate;
use crate::model::diagnostic::{CoverageStatus, ScanDiagnostic};
use crate::model::plan::Plan;
use crate::scanners::ScanResult;

/// Builds and manages execution plans from scan results.
#[derive(Debug, Default)]
pub struct PlanBuilder {
    candidates: Vec<Candidate>,
    diagnostics: Vec<ScanDiagnostic>,
    coverage_status: CoverageStatus,
    auto_approve_safe: bool,
}

impl PlanBuilder {
    pub fn new() -> Self {
        Self {
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            coverage_status: CoverageStatus::Complete,
            auto_approve_safe: false,
        }
    }

    pub fn with_auto_approve_safe(mut self, auto_approve: bool) -> Self {
        self.auto_approve_safe = auto_approve;
        self
    }

    pub fn add_scan_result(mut self, result: ScanResult) -> Self {
        self.candidates.extend(result.candidates);
        self.diagnostics.extend(result.diagnostics);
        self.coverage_status = self.coverage_status.merge(result.coverage_status);
        self
    }

    pub fn add_candidates(mut self, candidates: Vec<Candidate>) -> Self {
        self.candidates.extend(candidates);
        self
    }

    pub fn build(self) -> Plan {
        Plan::from_candidates(
            self.candidates,
            self.coverage_status,
            self.diagnostics,
            self.auto_approve_safe,
        )
    }
}
