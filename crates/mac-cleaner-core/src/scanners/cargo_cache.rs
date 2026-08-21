use std::path::PathBuf;

use crate::model::candidate::{Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner for Cargo registry crate archive cache and git checkouts.
#[derive(Debug, Default, Clone)]
pub struct CargoCacheScanner;

impl CargoCacheScanner {
    pub fn new() -> Self {
        Self
    }

    pub fn cargo_home() -> Option<PathBuf> {
        if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
            Some(PathBuf::from(cargo_home))
        } else if let Some(home) = std::env::var_os("HOME") {
            Some(PathBuf::from(home).join(".cargo"))
        } else {
            None
        }
    }
}

impl Scanner for CargoCacheScanner {
    fn name(&self) -> &'static str {
        "cargo_cache"
    }

    fn description(&self) -> &'static str {
        "Cargo registry package cache (.crate archives) and git checkouts"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        let cargo_dir = match Self::cargo_home() {
            Some(d) => d,
            None => return Ok(result),
        };

        if !cargo_dir.exists() {
            return Ok(result);
        }

        // 1. ~/.cargo/registry/cache (contains .crate files)
        let registry_cache = cargo_dir.join("registry").join("cache");
        if registry_cache.exists() {
            if let Ok(entries) = std::fs::read_dir(&registry_cache) {
                for entry_res in entries.flatten() {
                    let path = entry_res.path();
                    let file_name = entry_res.file_name();
                    let name_str = file_name.to_string_lossy();

                    if name_str.starts_with('.') {
                        continue;
                    }

                    let mut size_diagnostics = Vec::new();
                    let size = match calculate_path_size_with_diagnostics(
                        &path,
                        &mut size_diagnostics,
                        self.name(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            result.diagnostics.push(ScanDiagnostic::io_error(
                                Some(path.clone()),
                                self.name(),
                                format!("Failed to size {}: {}", path.display(), e),
                            ));
                            continue;
                        }
                    };

                    result.diagnostics.extend(size_diagnostics);

                    if size.is_zero() {
                        continue;
                    }

                    if let Ok(fp) = RevalidationFingerprint::from_path(&path) {
                        let candidate = Candidate::new(
                            path.clone(),
                            self.name(),
                            RiskTier::Tier1ReDownloadable,
                            ExecutionStrategy::NativeTrash,
                            size,
                            fp,
                            format!("Cargo registry downloaded crate archive ({})", name_str),
                        )
                        .with_metadata("cache_type", "registry_cache")
                        .with_metadata("safe_for_auto_clean", "true");

                        result.candidates.push(candidate);
                    }
                }
            }
        }

        // 2. ~/.cargo/git/checkouts
        let git_checkouts = cargo_dir.join("git").join("checkouts");
        if git_checkouts.exists() {
            if let Ok(entries) = std::fs::read_dir(&git_checkouts) {
                for entry_res in entries.flatten() {
                    let path = entry_res.path();
                    let file_name = entry_res.file_name();
                    let name_str = file_name.to_string_lossy();

                    if name_str.starts_with('.') {
                        continue;
                    }

                    let mut size_diagnostics = Vec::new();
                    let size = match calculate_path_size_with_diagnostics(
                        &path,
                        &mut size_diagnostics,
                        self.name(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            result.diagnostics.push(ScanDiagnostic::io_error(
                                Some(path.clone()),
                                self.name(),
                                format!("Failed to size {}: {}", path.display(), e),
                            ));
                            continue;
                        }
                    };

                    result.diagnostics.extend(size_diagnostics);

                    if size.is_zero() {
                        continue;
                    }

                    if let Ok(fp) = RevalidationFingerprint::from_path(&path) {
                        let candidate = Candidate::new(
                            path.clone(),
                            self.name(),
                            RiskTier::Tier1ReDownloadable,
                            ExecutionStrategy::NativeTrash,
                            size,
                            fp,
                            format!("Cargo git dependency checkout ({})", name_str),
                        )
                        .with_metadata("cache_type", "git_checkouts")
                        .with_metadata("safe_for_auto_clean", "true");

                        result.candidates.push(candidate);
                    }
                }
            }
        }

        // 3. ~/.cargo/registry/src (unpacked crates)
        let registry_src = cargo_dir.join("registry").join("src");
        if registry_src.exists() {
            if let Ok(entries) = std::fs::read_dir(&registry_src) {
                for entry_res in entries.flatten() {
                    let path = entry_res.path();
                    let file_name = entry_res.file_name();
                    let name_str = file_name.to_string_lossy();

                    if name_str.starts_with('.') {
                        continue;
                    }

                    let mut size_diagnostics = Vec::new();
                    let size = match calculate_path_size_with_diagnostics(
                        &path,
                        &mut size_diagnostics,
                        self.name(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            result.diagnostics.push(ScanDiagnostic::io_error(
                                Some(path.clone()),
                                self.name(),
                                format!("Failed to size {}: {}", path.display(), e),
                            ));
                            continue;
                        }
                    };

                    result.diagnostics.extend(size_diagnostics);

                    if size.is_zero() {
                        continue;
                    }

                    if let Ok(fp) = RevalidationFingerprint::from_path(&path) {
                        let candidate = Candidate::new(
                            path.clone(),
                            self.name(),
                            RiskTier::Tier1ReDownloadable,
                            ExecutionStrategy::NativeTrash,
                            size,
                            fp,
                            format!("Cargo registry extracted source ({})", name_str),
                        )
                        .with_metadata("cache_type", "registry_src")
                        .with_metadata("safe_for_auto_clean", "true");

                        result.candidates.push(candidate);
                    }
                }
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
    fn test_cargo_cache_scanner_basic() {
        let scanner = CargoCacheScanner::new();
        assert_eq!(scanner.name(), "cargo_cache");
        assert_eq!(
            scanner.description(),
            "Cargo registry package cache (.crate archives) and git checkouts"
        );

        let temp = tempdir().unwrap();
        let reg_cache = temp.path().join("registry").join("cache").join("github.com-1ecc6299db9ec823");
        std::fs::create_dir_all(&reg_cache).unwrap();
        std::fs::write(reg_cache.join("serde-1.0.0.crate"), vec![0x12; 1024]).unwrap();

        let fp = RevalidationFingerprint::from_path(&reg_cache).unwrap();
        assert!(fp.is_dir);
    }
}
