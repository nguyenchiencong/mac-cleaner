use std::path::PathBuf;

use crate::model::candidate::{Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner for npm package cache (`~/.npm/_cacache`).
#[derive(Debug, Default, Clone)]
pub struct NpmCacheScanner;

impl NpmCacheScanner {
    pub fn new() -> Self {
        Self
    }

    pub fn npm_cacache_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home).join(".npm").join("_cacache");
            paths.push(p);
        }

        paths
    }
}

impl Scanner for NpmCacheScanner {
    fn name(&self) -> &'static str {
        "npm_cache"
    }

    fn description(&self) -> &'static str {
        "npm content-addressable package cache (~/.npm/_cacache)"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        for cache_path in Self::npm_cacache_paths() {
            if !cache_path.exists() {
                continue;
            }

            let mut size_diagnostics = Vec::new();
            let size = match calculate_path_size_with_diagnostics(
                &cache_path,
                &mut size_diagnostics,
                self.name(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    result.diagnostics.push(ScanDiagnostic::io_error(
                        Some(cache_path.clone()),
                        self.name(),
                        format!("Failed to calculate size for {}: {}", cache_path.display(), e),
                    ));
                    continue;
                }
            };

            result.diagnostics.extend(size_diagnostics);

            if size.is_zero() {
                continue;
            }

            if let Ok(fp) = RevalidationFingerprint::from_path(&cache_path) {
                let candidate = Candidate::new(
                    cache_path.clone(),
                    self.name(),
                    RiskTier::Tier1ReDownloadable,
                    ExecutionStrategy::NativeTrash,
                    size,
                    fp,
                    "npm content-addressable package cache (_cacache)",
                )
                .with_metadata("cache_type", "npm_cacache")
                .with_metadata("safe_for_auto_clean", "true");

                result.candidates.push(candidate);
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_npm_cache_scanner_basic() {
        let scanner = NpmCacheScanner::new();
        assert_eq!(scanner.name(), "npm_cache");
        assert_eq!(
            scanner.description(),
            "npm content-addressable package cache (~/.npm/_cacache)"
        );

        let temp = tempdir().unwrap();
        let cacache = temp.path().join("_cacache").join("content-v2");
        std::fs::create_dir_all(&cacache).unwrap();
        std::fs::write(cacache.join("data.bin"), vec![0x33; 2048]).unwrap();

        let fp = RevalidationFingerprint::from_path(&cacache).unwrap();
        assert!(fp.is_dir);
    }
}
