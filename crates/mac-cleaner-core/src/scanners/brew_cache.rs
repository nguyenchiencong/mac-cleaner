use std::path::PathBuf;

use crate::model::candidate::{AllocatedSize, Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner discovering Homebrew package and formula download caches.
#[derive(Debug, Default, Clone)]
pub struct BrewCacheScanner;

impl BrewCacheScanner {
    pub fn new() -> Self {
        Self
    }

    pub fn cache_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Some(brew_cache) = std::env::var_os("HOMEBREW_CACHE") {
            paths.push(PathBuf::from(brew_cache));
        }

        if let Some(home) = std::env::var_os("HOME") {
            paths.push(
                PathBuf::from(home)
                    .join("Library")
                    .join("Caches")
                    .join("Homebrew"),
            );
        }

        paths.push(PathBuf::from("/Library/Caches/Homebrew"));

        paths
    }
}

impl Scanner for BrewCacheScanner {
    fn name(&self) -> &'static str {
        "brew_cache"
    }

    fn description(&self) -> &'static str {
        "Homebrew bottle and formula download cache"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());
        let mut inspected_dirs = std::collections::HashSet::new();

        for cache_dir in Self::cache_paths() {
            let canonical = match cache_dir.canonicalize() {
                Ok(c) => c,
                Err(_) => cache_dir.clone(),
            };

            if !inspected_dirs.insert(canonical) || !cache_dir.exists() {
                continue;
            }

            let entries = match std::fs::read_dir(&cache_dir) {
                Ok(e) => e,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        result.diagnostics.push(ScanDiagnostic::permission_denied(
                            cache_dir,
                            self.name(),
                            format!("Permission denied reading Homebrew cache: {}", e),
                        ));
                        result.coverage_status = result
                            .coverage_status
                            .merge(crate::model::diagnostic::CoverageStatus::Degraded);
                    }
                    continue;
                }
            };

            let mut loose_files_size = AllocatedSize::zero();
            let mut loose_files_count = 0usize;

            for entry_res in entries {
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(e) => {
                        result.diagnostics.push(ScanDiagnostic::io_error(
                            Some(cache_dir.clone()),
                            self.name(),
                            format!("Error reading Homebrew cache entry: {}", e),
                        ));
                        continue;
                    }
                };

                let path = entry.path();
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();

                if name_str.starts_with('.') {
                    continue;
                }

                let file_type = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
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
                                format!("Failed to calculate size for {}: {}", path.display(), e),
                            ));
                            continue;
                        }
                    };

                    result.diagnostics.extend(size_diagnostics);

                    if size.is_zero() {
                        continue;
                    }

                    if let Ok(fp) = RevalidationFingerprint::from_path(&path) {
                        let desc = if name_str == "downloads" {
                            "Homebrew downloaded formula packages (bottles)".to_string()
                        } else if name_str == "Cask" {
                            "Homebrew Cask installer caches".to_string()
                        } else if name_str == "api" {
                            "Homebrew API formula metadata cache".to_string()
                        } else {
                            format!("Homebrew cache directory ({})", name_str)
                        };

                        let candidate = Candidate::new(
                            path.clone(),
                            self.name(),
                            RiskTier::Tier1ReDownloadable,
                            ExecutionStrategy::NativeTrash,
                            size,
                            fp,
                            desc,
                        )
                        .with_metadata("item_name", name_str.to_string())
                        .with_metadata("safe_for_auto_clean", "true");

                        result.candidates.push(candidate);
                    }
                } else {
                    // Regular file: calculate size
                    if let Ok(meta) = entry.metadata() {
                        let sz = crate::scan::size::calculate_path_size(&path).unwrap_or_else(|_| {
                            AllocatedSize::new(meta.len(), meta.len())
                        });
                        loose_files_size += sz;
                        loose_files_count += 1;
                    }
                }
            }

            // If there are loose files in Homebrew cache dir, add them as a single candidate
            if loose_files_count > 0 && !loose_files_size.is_zero() {
                if let Ok(fp) = RevalidationFingerprint::from_path(&cache_dir) {
                    let candidate = Candidate::new(
                        cache_dir.clone(),
                        self.name(),
                        RiskTier::Tier1ReDownloadable,
                        ExecutionStrategy::NativeTrash,
                        loose_files_size,
                        fp,
                        format!("Homebrew formula manifests and index files ({} files)", loose_files_count),
                    )
                    .with_metadata("item_name", "manifests")
                    .with_metadata("file_count", loose_files_count.to_string())
                    .with_metadata("safe_for_auto_clean", "true");

                    result.candidates.push(candidate);
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
    fn test_brew_cache_scanner_basic() {
        let scanner = BrewCacheScanner::new();
        assert_eq!(scanner.name(), "brew_cache");
        assert_eq!(scanner.description(), "Homebrew bottle and formula download cache");

        let temp = tempdir().unwrap();
        let brew_dir = temp.path().join("Homebrew");
        let downloads = brew_dir.join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("bottle--1.0.tar.gz"), vec![0x99; 2048]).unwrap();

        let fp = RevalidationFingerprint::from_path(&downloads).unwrap();
        assert!(fp.is_dir);
    }
}
