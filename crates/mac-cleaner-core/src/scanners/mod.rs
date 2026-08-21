use std::collections::HashMap;
use std::sync::Arc;
use rayon::prelude::*;
use thiserror::Error;

use crate::config::{ConfigProfile, WorkspaceRoot};
use crate::model::candidate::Candidate;
use crate::model::diagnostic::{CoverageStatus, ScanDiagnostic};

pub mod brew_cache;
pub mod cargo_cache;
pub mod node_modules;
pub mod npm_cache;
pub mod rust_target;
pub mod unavailable_simulators;
pub mod user_caches;
pub mod xcode_derived_data;

pub use brew_cache::BrewCacheScanner;
pub use cargo_cache::CargoCacheScanner;
pub use node_modules::NodeModulesScanner;
pub use npm_cache::NpmCacheScanner;
pub use rust_target::RustTargetScanner;
pub use unavailable_simulators::UnavailableSimulatorsScanner;
pub use user_caches::UserCachesScanner;
pub use xcode_derived_data::XcodeDerivedDataScanner;

#[derive(Error, Debug)]
pub enum ScannerError {
    #[error("Scanner '{0}' execution failed: {1}")]
    ExecutionFailed(String, String),

    #[error("I/O error during scan: {0}")]
    Io(#[from] std::io::Error),
}

/// Context provided to scanners during discovery.
#[derive(Debug, Clone)]
pub struct ScanContext {
    pub workspace_roots: Vec<WorkspaceRoot>,
    pub exclusions: Vec<String>,
    pub age_threshold_days: Option<u32>,
    pub custom_options: HashMap<String, String>,
}

impl ScanContext {
    pub fn from_config(config: &ConfigProfile) -> Self {
        Self {
            workspace_roots: config.workspace_roots.clone(),
            exclusions: config.exclusions.clone(),
            age_threshold_days: config.age_threshold_days,
            custom_options: HashMap::new(),
        }
    }
}

/// Discovered candidates and diagnostic information produced by a scanner.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub scanner_name: String,
    pub candidates: Vec<Candidate>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub coverage_status: CoverageStatus,
}

impl ScanResult {
    pub fn new(scanner_name: impl Into<String>) -> Self {
        Self {
            scanner_name: scanner_name.into(),
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            coverage_status: CoverageStatus::Complete,
        }
    }

    pub fn with_candidate(mut self, candidate: Candidate) -> Self {
        self.candidates.push(candidate);
        self
    }

    pub fn with_diagnostic(mut self, diagnostic: ScanDiagnostic) -> Self {
        if matches!(
            diagnostic.kind,
            crate::model::diagnostic::DiagnosticKind::PermissionDenied
                | crate::model::diagnostic::DiagnosticKind::IncompleteCoverage
        ) {
            self.coverage_status = self.coverage_status.merge(CoverageStatus::Degraded);
        }
        self.diagnostics.push(diagnostic);
        self
    }

    pub fn merge(mut self, other: ScanResult) -> Self {
        self.candidates.extend(other.candidates);
        self.diagnostics.extend(other.diagnostics);
        self.coverage_status = self.coverage_status.merge(other.coverage_status);
        self
    }
}

/// Unified inspection module responsible for discovering candidates without mutating disk state.
pub trait Scanner: Send + Sync {
    /// Canonical scanner identifier (e.g. "rust_target", "node_modules", "xcode_derived_data").
    fn name(&self) -> &'static str;

    /// Human-readable description of what this scanner inspects.
    fn description(&self) -> &'static str;

    /// Inspects the environment and produces discovered candidates and diagnostics.
    fn scan(&self, ctx: &ScanContext) -> Result<ScanResult, ScannerError>;
}

/// Registry managing active scanners and parallel execution.
#[derive(Default, Clone)]
pub struct ScannerRegistry {
    scanners: Vec<Arc<dyn Scanner>>,
}

impl ScannerRegistry {
    pub fn new() -> Self {
        Self {
            scanners: Vec::new(),
        }
    }

    /// Initializes registry with the full concrete scanner suite.
    pub fn default_registry() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(RustTargetScanner::new()));
        registry.register(Arc::new(NodeModulesScanner::new()));
        registry.register(Arc::new(XcodeDerivedDataScanner::new()));
        registry.register(Arc::new(UnavailableSimulatorsScanner::new()));
        registry.register(Arc::new(BrewCacheScanner::new()));
        registry.register(Arc::new(CargoCacheScanner::new()));
        registry.register(Arc::new(NpmCacheScanner::new()));
        registry.register(Arc::new(UserCachesScanner::new()));
        registry
    }

    pub fn register(&mut self, scanner: Arc<dyn Scanner>) {
        self.scanners.push(scanner);
    }

    pub fn scanners(&self) -> &[Arc<dyn Scanner>] {
        &self.scanners
    }

    pub fn execute_all(&self, ctx: &ScanContext) -> Vec<Result<ScanResult, ScannerError>> {
        self.scanners
            .par_iter()
            .map(|scanner| scanner.scan(ctx))
            .collect()
    }

    pub fn execute_filtered<F>(
        &self,
        ctx: &ScanContext,
        filter: F,
    ) -> Vec<Result<ScanResult, ScannerError>>
    where
        F: Fn(&dyn Scanner) -> bool + Sync + Send,
    {
        self.scanners
            .par_iter()
            .filter(|s| filter(s.as_ref()))
            .map(|scanner| scanner.scan(ctx))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_initialization() {
        let registry = ScannerRegistry::default_registry();
        assert_eq!(registry.scanners().len(), 8);

        let scanner_names: Vec<&str> = registry.scanners().iter().map(|s| s.name()).collect();
        assert!(scanner_names.contains(&"rust_target"));
        assert!(scanner_names.contains(&"node_modules"));
        assert!(scanner_names.contains(&"xcode_derived_data"));
        assert!(scanner_names.contains(&"unavailable_simulators"));
        assert!(scanner_names.contains(&"brew_cache"));
        assert!(scanner_names.contains(&"cargo_cache"));
        assert!(scanner_names.contains(&"npm_cache"));
        assert!(scanner_names.contains(&"user_caches"));
    }
}
