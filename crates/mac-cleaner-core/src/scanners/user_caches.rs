use std::path::PathBuf;

use crate::model::candidate::{Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner inspecting safe user application cache directories under `~/Library/Caches`.
#[derive(Debug, Default, Clone)]
pub struct UserCachesScanner;

impl UserCachesScanner {
    pub fn new() -> Self {
        Self
    }

    pub fn user_caches_dir() -> Option<PathBuf> {
        if let Some(home) = std::env::var_os("HOME") {
            Some(PathBuf::from(home).join("Library").join("Caches"))
        } else {
            None
        }
    }

    /// Checks if a cache directory entry is safe and recommended for inspection.
    pub fn is_allowlisted_cache(name: &str) -> bool {
        let name_lower = name.to_lowercase();

        // Critical / System exclusions (never touch or scan)
        if name.starts_with('.')
            || name == "Homebrew"
            || name.starts_with("com.apple.Safari")
            || name.starts_with("com.apple.CloudKit")
            || name.starts_with("com.apple.keychain")
            || name.starts_with("com.apple.bird")
            || name.starts_with("com.apple.findmy")
            || name.starts_with("com.apple.passd")
            || name.starts_with("com.apple.identityservices")
            || name.starts_with("com.apple.AddressBook")
            || name.starts_with("com.apple.homed")
            || name.starts_with("com.apple.accountsd")
            || name.starts_with("com.apple.Preferences")
            || name.starts_with("com.apple.Core")
            || name.starts_with("com.apple.security")
            || name.starts_with("com.apple.ap.")
            || name.starts_with("com.apple.commerce")
            || name.starts_with("com.apple.mobileme")
            || name.starts_with("com.apple.icloud")
            || name.starts_with("com.apple.dt.Xcode")
            || name.starts_with("FamilyCircle")
            || name.starts_with("GeoServices")
        {
            return false;
        }

        // Known safe third-party / dev / app prefixes
        let safe_prefixes = [
            "com.google.chrome",
            "google",
            "org.mozilla.firefox",
            "firefox",
            "com.brave.browser",
            "company.thebrowser.browser", // Arc
            "com.operasoftware.",
            "com.microsoft.edgemac",
            "com.microsoft.vscode",
            "vscode",
            "com.github.githubclient",
            "com.tinyspeck.slackmacgap",
            "slack",
            "com.spotify.client",
            "spotify",
            "com.docker.docker",
            "com.postmanlabs.mac",
            "postman",
            "com.figma.desktop",
            "figma",
            "jetbrains",
            "com.jetbrains.",
            "com.electron.",
            "cursor",
            "yarn",
            "pip",
            "pypoetry",
            "cocoapods",
            "deno",
            "com.colliderli.iina",
            "org.videolan.vlc",
            "com.discordapp.discord",
            "discord",
            "us.zoom.xos",
            "ru.keepcoder.telegram",
            "telegram",
            "com.sublimetext.",
            "com.raycast.macos",
            "com.linear",
            "notion",
            "ms-vscode",
        ];

        if safe_prefixes.iter().any(|p| name_lower.starts_with(p)) {
            return true;
        }

        // Include any non-apple third party bundle ID pattern (e.g. `com.something.*` or `org.something.*`)
        if (name.starts_with("com.") || name.starts_with("org.") || name.starts_with("io.") || name.starts_with("net."))
            && !name.starts_with("com.apple.")
        {
            return true;
        }

        false
    }
}

impl Scanner for UserCachesScanner {
    fn name(&self) -> &'static str {
        "user_caches"
    }

    fn description(&self) -> &'static str {
        "Allowlisted user application caches in ~/Library/Caches"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        let caches_dir = match Self::user_caches_dir() {
            Some(d) => d,
            None => return Ok(result),
        };

        if !caches_dir.exists() {
            return Ok(result);
        }

        let entries = match std::fs::read_dir(&caches_dir) {
            Ok(e) => e,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    result.diagnostics.push(ScanDiagnostic::permission_denied(
                        caches_dir,
                        self.name(),
                        format!("Permission denied reading ~/Library/Caches: {}", e),
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
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        result.diagnostics.push(ScanDiagnostic::permission_denied(
                            caches_dir.clone(),
                            self.name(),
                            format!("Permission denied reading cache entry: {}", e),
                        ));
                        result.coverage_status = result
                            .coverage_status
                            .merge(crate::model::diagnostic::CoverageStatus::Degraded);
                    } else {
                        result.diagnostics.push(ScanDiagnostic::io_error(
                            Some(caches_dir.clone()),
                            self.name(),
                            format!("Error reading cache entry: {}", e),
                        ));
                    }
                    continue;
                }
            };

            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if !Self::is_allowlisted_cache(&name_str) {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        result.diagnostics.push(ScanDiagnostic::permission_denied(
                            path,
                            self.name(),
                            "Permission denied checking entry metadata",
                        ));
                        result.coverage_status = result
                            .coverage_status
                            .merge(crate::model::diagnostic::CoverageStatus::Degraded);
                    }
                    continue;
                }
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
                        format!("Failed to calculate size for {}: {}", path.display(), e),
                    ));
                    continue;
                }
            };

            result.diagnostics.extend(size_diagnostics);

            // Skip zero or tiny (< 10 KB) empty cache directories to reduce noise
            if size.logical_bytes < 10 * 1024 && size.allocated_bytes < 10 * 1024 {
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
                    format!("Application cache ({})", name_str),
                )
                .with_metadata("app_bundle_id", name_str.to_string())
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

    #[test]
    fn test_allowlist_filtering() {
        assert!(UserCachesScanner::is_allowlisted_cache("com.google.Chrome"));
        assert!(UserCachesScanner::is_allowlisted_cache("com.spotify.client"));
        assert!(UserCachesScanner::is_allowlisted_cache("com.microsoft.VSCode"));
        assert!(UserCachesScanner::is_allowlisted_cache("yarn"));
        assert!(UserCachesScanner::is_allowlisted_cache("io.github.myapp"));

        // Critical / system caches must return false
        assert!(!UserCachesScanner::is_allowlisted_cache("com.apple.Safari"));
        assert!(!UserCachesScanner::is_allowlisted_cache("com.apple.CloudKit"));
        assert!(!UserCachesScanner::is_allowlisted_cache("com.apple.keychain"));
        assert!(!UserCachesScanner::is_allowlisted_cache("Homebrew"));
    }
}
