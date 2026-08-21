use std::path::PathBuf;
use std::time::Duration;
use serde_json::Value;

use crate::model::candidate::{AllocatedSize, Candidate, ExecutionStrategy, RevalidationFingerprint, RiskTier};
use crate::model::diagnostic::ScanDiagnostic;
use crate::platform::process::ProcessExecutor;
use crate::scan::size::calculate_path_size_with_diagnostics;
use crate::scanners::{ScanContext, ScanResult, Scanner, ScannerError};

/// Scanner discovering unavailable simulator devices using `xcrun simctl`.
#[derive(Debug, Default, Clone)]
pub struct UnavailableSimulatorsScanner {
    process_executor: ProcessExecutor,
}

impl UnavailableSimulatorsScanner {
    pub fn new() -> Self {
        Self {
            process_executor: ProcessExecutor::new(),
        }
    }

    pub fn devices_dir() -> Option<PathBuf> {
        if let Some(home) = std::env::var_os("HOME") {
            Some(
                PathBuf::from(home)
                    .join("Library")
                    .join("Developer")
                    .join("CoreSimulator")
                    .join("Devices"),
            )
        } else {
            None
        }
    }
}

impl Scanner for UnavailableSimulatorsScanner {
    fn name(&self) -> &'static str {
        "unavailable_simulators"
    }

    fn description(&self) -> &'static str {
        "Unavailable iOS and watchOS simulator devices and runtimes"
    }

    fn scan(&self, _ctx: &ScanContext) -> Result<ScanResult, ScannerError> {
        let mut result = ScanResult::new(self.name());

        // Attempt to execute `xcrun simctl list -j devices`
        let output = match self.process_executor.execute(
            "xcrun",
            &["simctl", "list", "-j", "devices"],
            Some(Duration::from_secs(15)),
        ) {
            Ok(out) => out,
            Err(e) => {
                // If xcrun is missing (non-macOS or no Xcode CLI tools), exit gracefully
                tracing::debug!("xcrun simctl unavailable: {}", e);
                return Ok(result);
            }
        };

        if !output.success {
            tracing::debug!("xcrun simctl returned non-zero: {}", output.stderr);
            return Ok(result);
        }

        let json_val: Value = match serde_json::from_str(&output.stdout) {
            Ok(v) => v,
            Err(e) => {
                result.diagnostics.push(ScanDiagnostic::io_error(
                    None,
                    self.name(),
                    format!("Failed to parse xcrun simctl JSON: {}", e),
                ));
                return Ok(result);
            }
        };

        let mut unavailable_count = 0usize;
        let mut total_size = AllocatedSize::zero();
        let mut unavailable_udids = Vec::new();

        if let Some(devices_obj) = json_val.get("devices").and_then(|d| d.as_object()) {
            for (_runtime, device_list) in devices_obj {
                if let Some(devices) = device_list.as_array() {
                    for dev in devices {
                        let is_available = dev
                            .get("isAvailable")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        let availability_error = dev.get("availabilityError");

                        if !is_available || availability_error.is_some() {
                            unavailable_count += 1;
                            if let Some(udid) = dev.get("udid").and_then(|u| u.as_str()) {
                                unavailable_udids.push(udid.to_string());
                            }

                            if let Some(data_path_str) = dev.get("dataPath").and_then(|p| p.as_str()) {
                                let p = PathBuf::from(data_path_str);
                                if p.exists() {
                                    let mut diag = Vec::new();
                                    if let Ok(sz) = calculate_path_size_with_diagnostics(&p, &mut diag, self.name()) {
                                        total_size += sz;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if unavailable_count > 0 {
            let devices_dir = Self::devices_dir().unwrap_or_else(|| PathBuf::from("/Library/Developer/CoreSimulator/Devices"));
            let fp = RevalidationFingerprint::from_path(&devices_dir).unwrap_or(RevalidationFingerprint {
                dev: 0,
                ino: 0,
                size: total_size.logical_bytes,
                mtime_sec: 0,
                mtime_nsec: 0,
                nlink: 1,
                is_dir: true,
            });

            let cmd = vec![
                "xcrun".to_string(),
                "simctl".to_string(),
                "delete".to_string(),
                "unavailable".to_string(),
            ];
            let cmd_json = serde_json::to_string(&cmd).unwrap_or_default();

            let description = format!(
                "Unavailable simulator runtimes and devices ({} devices)",
                unavailable_count
            );

            let candidate = Candidate::new(
                devices_dir,
                self.name(),
                RiskTier::Tier1ReDownloadable,
                ExecutionStrategy::ExternalCommand,
                total_size,
                fp,
                description,
            )
            .with_metadata("command", cmd_json)
            .with_metadata("unavailable_count", unavailable_count.to_string())
            .with_metadata("unavailable_udids", unavailable_udids.join(","))
            .with_metadata("safe_for_auto_clean", "true")
            .with_metadata("restorable", "false");

            result.candidates.push(candidate);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unavailable_simulators_metadata() {
        let scanner = UnavailableSimulatorsScanner::new();
        assert_eq!(scanner.name(), "unavailable_simulators");
        assert_eq!(
            scanner.description(),
            "Unavailable iOS and watchOS simulator devices and runtimes"
        );
    }
}
