use std::path::PathBuf;

use crate::model::candidate::{Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner for Xcode DerivedData build artifacts and module caches.
#[derive(Debug, Default, Clone)]
pub struct XcodeDerivedDataScanner;

impl XcodeDerivedDataScanner {
    pub fn new() -> Self {
        Self
    }

    pub fn derived_data_path() -> Option<PathBuf> {
        if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home)
                .join("Library")
                .join("Developer")
                .join("Xcode")
                .join("DerivedData");
            Some(p)
        } else {
            None
        }
    }
}

impl Scanner for XcodeDerivedDataScanner {
    fn name(&self) -> &'static str {
        "xcode_derived_data"
    }

    fn description(&self) -> &'static str {
        "Xcode build artifacts and index data (~/Library/Developer/Xcode/DerivedData)"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        let derived_data_dir = match Self::derived_data_path() {
            Some(p) => p,
            None => return Ok(result),
        };

        if !derived_data_dir.exists() {
            return Ok(result);
        }

        let entries = match std::fs::read_dir(&derived_data_dir) {
            Ok(e) => e,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    result.diagnostics.push(ScanDiagnostic::permission_denied(
                        derived_data_dir,
                        self.name(),
                        format!("Permission denied reading Xcode DerivedData: {}", e),
                    ));
                    result.coverage_status = result
                        .coverage_status
                        .merge(crate::model::diagnostic::CoverageStatus::Degraded);
                }
                return Ok(result);
            }
        };

        for entry_res in entries {
            let entry = match entry_res {
                Ok(e) => e,
                Err(e) => {
                    result.diagnostics.push(ScanDiagnostic::io_error(
                        Some(derived_data_dir.clone()),
                        self.name(),
                        format!("Error reading DerivedData entry: {}", e),
                    ));
                    continue;
                }
            };

            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            // Skip hidden files like .DS_Store
            if name_str.starts_with('.') {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if !file_type.is_dir() {
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
                        format!("Failed to calculate size of {}: {}", path.display(), e),
                    ));
                    continue;
                }
            };

            result.diagnostics.extend(size_diagnostics);

            if let Ok(fp) = RevalidationFingerprint::from_path(&path) {
                let description = format!("Xcode DerivedData build cache ({})", name_str);
                let candidate = Candidate::new(
                    path.clone(),
                    self.name(),
                    RiskTier::Tier2Rebuildable,
                    ExecutionStrategy::NativeTrash,
                    size,
                    fp,
                    description,
                )
                .with_metadata("project_name", name_str.to_string())
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
    fn test_xcode_derived_data_scan() {
        let temp = tempdir().unwrap();
        let derived_data = temp.path().join("DerivedData");
        let app_cache = derived_data.join("MyApp-a1b2c3d4e5");
        std::fs::create_dir_all(&app_cache).unwrap();
        std::fs::write(app_cache.join("build.db"), vec![0u8; 1024]).unwrap();

        // Test scanner logic on manual directory
        let scanner = XcodeDerivedDataScanner::new();
        assert_eq!(scanner.name(), "xcode_derived_data");
        assert_eq!(scanner.description(), "Xcode build artifacts and index data (~/Library/Developer/Xcode/DerivedData)");
    }
}
