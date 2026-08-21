use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::model::candidate::{Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner for Rust build artifacts (`target/` directories).
#[derive(Debug, Default, Clone)]
pub struct RustTargetScanner;

impl RustTargetScanner {
    pub fn new() -> Self {
        Self
    }

    /// Computes project inactivity age in days by inspecting modification times
    /// of project definition and source files (`Cargo.toml`, `Cargo.lock`, `src/`).
    fn calculate_inactivity_days(project_root: &Path) -> u64 {
        let mut newest_mtime = SystemTime::UNIX_EPOCH;

        // Check Cargo.toml
        if let Ok(meta) = std::fs::symlink_metadata(project_root.join("Cargo.toml")) {
            if let Ok(mtime) = meta.modified() {
                if mtime > newest_mtime {
                    newest_mtime = mtime;
                }
            }
        }

        // Check Cargo.lock
        if let Ok(meta) = std::fs::symlink_metadata(project_root.join("Cargo.lock")) {
            if let Ok(mtime) = meta.modified() {
                if mtime > newest_mtime {
                    newest_mtime = mtime;
                }
            }
        }

        // Check src directory files (bounded inspection)
        let src_dir = project_root.join("src");
        if src_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&src_dir) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if mtime > newest_mtime {
                                newest_mtime = mtime;
                            }
                        }
                    }
                }
            }
        }

        if let Ok(elapsed) = SystemTime::now().duration_since(newest_mtime) {
            elapsed.as_secs() / 86400
        } else {
            0
        }
    }

    /// Extracts package name from Cargo.toml or directory name.
    fn extract_project_name(project_root: &Path) -> String {
        let cargo_toml = project_root.join("Cargo.toml");
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name") {
                    if let Some((_, val)) = trimmed.split_once('=') {
                        let name = val.trim().trim_matches('"').trim_matches('\'').trim();
                        if !name.is_empty() {
                            return name.to_string();
                        }
                    }
                }
            }
        }

        project_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Rust Project".to_string())
    }

    fn scan_directory(
        &self,
        current_dir: &Path,
        current_depth: usize,
        max_depth: usize,
        ctx: &ScanContext,
        result: &mut ScanResult,
    ) {
        if current_depth > max_depth {
            return;
        }

        let entries = match std::fs::read_dir(current_dir) {
            Ok(e) => e,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    result.diagnostics.push(ScanDiagnostic::permission_denied(
                        current_dir.to_path_buf(),
                        self.name(),
                        format!("Cannot read directory: {}", e),
                    ));
                    result.coverage_status = result
                        .coverage_status
                        .merge(crate::model::diagnostic::CoverageStatus::Degraded);
                }
                return;
            }
        };

        for entry_res in entries {
            let entry = match entry_res {
                Ok(e) => e,
                Err(e) => {
                    result.diagnostics.push(ScanDiagnostic::io_error(
                        Some(current_dir.to_path_buf()),
                        self.name(),
                        format!("Error reading entry: {}", e),
                    ));
                    continue;
                }
            };

            let entry_path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            // Skip hidden directories and standard vcs / dependency caches
            if name_str.starts_with('.') || name_str == "node_modules" {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if !file_type.is_dir() {
                continue;
            }

            // Check if this directory is target/ belonging to a Cargo project
            if name_str == "target" {
                let project_root = current_dir;
                let cargo_toml = project_root.join("Cargo.toml");

                if cargo_toml.exists() {
                    // This is a Rust target directory!
                    let project_name = Self::extract_project_name(project_root);
                    let inactive_days = Self::calculate_inactivity_days(project_root);

                    let configured_threshold = ctx.age_threshold_days.unwrap_or(30) as u64;
                    let safe_for_auto_clean = inactive_days >= configured_threshold;

                    let mut size_diagnostics = Vec::new();
                    let size = match calculate_path_size_with_diagnostics(
                        &entry_path,
                        &mut size_diagnostics,
                        self.name(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            result.diagnostics.push(ScanDiagnostic::io_error(
                                Some(entry_path.clone()),
                                self.name(),
                                format!("Failed to compute target directory size: {}", e),
                            ));
                            continue;
                        }
                    };

                    result.diagnostics.extend(size_diagnostics);

                    if let Ok(fp) = RevalidationFingerprint::from_path(&entry_path) {
                        let candidate = Candidate::new(
                            entry_path.clone(),
                            self.name(),
                            RiskTier::Tier2Rebuildable,
                            ExecutionStrategy::NativeTrash,
                            size,
                            fp,
                            format!("Rust build target for {}", project_name),
                        )
                        .with_metadata("project_name", project_name)
                        .with_metadata("project_path", project_root.to_string_lossy().into_owned())
                        .with_metadata("inactive_days", inactive_days.to_string())
                        .with_metadata("safe_for_auto_clean", safe_for_auto_clean.to_string());

                        result.candidates.push(candidate);
                    }

                    // Invariant: Do not recurse into target/
                    continue;
                }
            }

            // Recurse into subdirectories
            self.scan_directory(
                &entry_path,
                current_depth + 1,
                max_depth,
                ctx,
                result,
            );
        }
    }
}

impl Scanner for RustTargetScanner {
    fn name(&self) -> &'static str {
        "rust_target"
    }

    fn description(&self) -> &'static str {
        "Rust project build artifacts (target/ directories)"
    }

    fn scan(&self, ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        let mut roots: Vec<PathBuf> = ctx
            .workspace_roots
            .iter()
            .map(|r| r.path.clone())
            .collect();

        if roots.is_empty() {
            if let Some(home) = std::env::var_os("HOME") {
                let home_path = PathBuf::from(home);
                let dev_path = home_path.join("Dev");
                if dev_path.exists() {
                    roots.push(dev_path);
                } else {
                    roots.push(home_path);
                }
            }
        }

        for root in roots {
            if !root.exists() {
                continue;
            }

            let max_depth = ctx
                .workspace_roots
                .iter()
                .find(|r| r.path == root)
                .and_then(|r| r.max_depth)
                .unwrap_or(5);

            self.scan_directory(&root, 0, max_depth, ctx, &mut result);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rust_target_scanner_discovery_and_inactivity() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("my_app");
        let target = project.join("target");
        let target_debug = target.join("debug");
        std::fs::create_dir_all(&target_debug).unwrap();

        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"my_app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(target_debug.join("binary.o"), vec![0xEE; 4096]).unwrap();

        let scanner = RustTargetScanner::new();
        let ctx = ScanContext {
            workspace_roots: vec![crate::config::WorkspaceRoot::new(dir.path())],
            exclusions: vec![],
            age_threshold_days: Some(0), // 0 days => immediately safe for auto-clean
            custom_options: Default::default(),
        };

        let result = scanner.scan(&ctx).unwrap();
        assert_eq!(result.candidates.len(), 1);
        let candidate = &result.candidates[0];
        assert_eq!(candidate.scanner_name, "rust_target");
        assert_eq!(candidate.risk_tier, RiskTier::Tier2Rebuildable);
        assert_eq!(candidate.metadata.get("project_name").unwrap(), "my_app");
        assert_eq!(candidate.metadata.get("safe_for_auto_clean").unwrap(), "true");
        assert!(candidate.size.logical_bytes >= 4096);
    }
}
