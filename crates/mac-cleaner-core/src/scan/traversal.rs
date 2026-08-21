use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

use crate::model::diagnostic::{CoverageStatus, ScanDiagnostic};

/// Known project marker categories identifying project boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMarkerType {
    Rust,
    Node,
    Go,
    JavaGradle,
    JavaMaven,
    Python,
    SwiftXcode,
    CMake,
    Git,
    Other,
}

/// A detected project boundary marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMarker {
    pub name: String,
    pub path: PathBuf,
    pub marker_type: ProjectMarkerType,
}

/// Configuration options for safe filesystem traversal.
#[derive(Debug, Clone)]
pub struct TraversalConfig {
    pub max_depth: Option<usize>,
    pub follow_symlinks: bool,
    pub ignore_hidden: bool,
    pub excluded_dir_names: Vec<String>,
}

impl Default for TraversalConfig {
    fn default() -> Self {
        Self {
            max_depth: None,
            // Invariant: Never follow symlinks by default (ADR 0002, 0006, 0010)
            follow_symlinks: false,
            ignore_hidden: false,
            excluded_dir_names: vec![
                ".Trash".to_string(),
                ".fseventsd".to_string(),
                ".Spotlight-V100".to_string(),
                ".DocumentRevisions-V100".to_string(),
                ".TemporaryItems".to_string(),
            ],
        }
    }
}

/// Checks if a directory is a Git repository root.
pub fn is_git_repository(dir: &Path) -> bool {
    let git_path = dir.join(".git");
    if let Ok(meta) = std::fs::symlink_metadata(&git_path) {
        // Can be a directory (.git) or a file (git worktree / submodule .git file)
        meta.is_dir() || meta.is_file()
    } else {
        false
    }
}

/// Detects all standard project markers inside a directory.
pub fn detect_project_markers(dir: &Path) -> Vec<ProjectMarker> {
    let mut markers = Vec::new();

    let candidates = [
        ("Cargo.toml", ProjectMarkerType::Rust),
        ("package.json", ProjectMarkerType::Node),
        ("pnpm-workspace.yaml", ProjectMarkerType::Node),
        ("go.mod", ProjectMarkerType::Go),
        ("pom.xml", ProjectMarkerType::JavaMaven),
        ("build.gradle", ProjectMarkerType::JavaGradle),
        ("build.gradle.kts", ProjectMarkerType::JavaGradle),
        ("settings.gradle", ProjectMarkerType::JavaGradle),
        ("pyproject.toml", ProjectMarkerType::Python),
        ("requirements.txt", ProjectMarkerType::Python),
        ("Pipfile", ProjectMarkerType::Python),
        ("setup.py", ProjectMarkerType::Python),
        ("Package.swift", ProjectMarkerType::SwiftXcode),
        ("CMakeLists.txt", ProjectMarkerType::CMake),
    ];

    for (marker_name, marker_type) in candidates {
        let p = dir.join(marker_name);
        if p.exists() || std::fs::symlink_metadata(&p).is_ok() {
            markers.push(ProjectMarker {
                name: marker_name.to_string(),
                path: p,
                marker_type,
            });
        }
    }

    if is_git_repository(dir) {
        markers.push(ProjectMarker {
            name: ".git".to_string(),
            path: dir.join(".git"),
            marker_type: ProjectMarkerType::Git,
        });
    }

    markers
}

/// Ascends the directory hierarchy from `start_path` to find the nearest project root containing any of `marker_files`.
pub fn find_project_root(start_path: &Path, marker_files: &[&str]) -> Option<PathBuf> {
    let mut current = if start_path.is_file() {
        start_path.parent()?
    } else {
        start_path
    };

    loop {
        for marker in marker_files {
            let candidate = current.join(marker);
            if candidate.exists() || std::fs::symlink_metadata(&candidate).is_ok() {
                return Some(current.to_path_buf());
            }
        }

        match current.parent() {
            Some(parent) if parent != current => {
                current = parent;
            }
            _ => break,
        }
    }

    None
}

/// Result of a safe directory traversal.
#[derive(Debug, Clone)]
pub struct TraversalResult {
    pub paths: Vec<PathBuf>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub coverage_status: CoverageStatus,
}

/// Safely walks a directory tree with bounded recursion and symlink safety.
pub fn walk_directory_safely(
    root: &Path,
    config: &TraversalConfig,
    scanner_name: &str,
) -> TraversalResult {
    let mut paths = Vec::new();
    let mut diagnostics = Vec::new();
    let mut coverage_status = CoverageStatus::Complete;

    walk_recursive(
        root,
        0,
        config,
        scanner_name,
        &mut paths,
        &mut diagnostics,
        &mut coverage_status,
    );

    TraversalResult {
        paths,
        diagnostics,
        coverage_status,
    }
}

fn walk_recursive(
    current: &Path,
    current_depth: usize,
    config: &TraversalConfig,
    scanner_name: &str,
    paths: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ScanDiagnostic>,
    coverage_status: &mut CoverageStatus,
) {
    if let Some(max_depth) = config.max_depth {
        if current_depth > max_depth {
            return;
        }
    }

    let meta = match std::fs::symlink_metadata(current) {
        Ok(m) => m,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                *coverage_status = coverage_status.merge(CoverageStatus::Degraded);
                diagnostics.push(ScanDiagnostic::permission_denied(
                    current.to_path_buf(),
                    scanner_name,
                    format!("Cannot access path metadata: {}", e),
                ));
            } else if e.kind() == std::io::ErrorKind::NotFound {
                diagnostics.push(ScanDiagnostic::path_vanished(
                    current.to_path_buf(),
                    scanner_name,
                    "Path disappeared during traversal",
                ));
            } else {
                diagnostics.push(ScanDiagnostic::io_error(
                    Some(current.to_path_buf()),
                    scanner_name,
                    e.to_string(),
                ));
            }
            return;
        }
    };

    let file_type = meta.file_type();

    // Invariant: If symlink and follow_symlinks is false, do not recurse into target
    if file_type.is_symlink() && !config.follow_symlinks {
        paths.push(current.to_path_buf());
        return;
    }

    paths.push(current.to_path_buf());

    if !file_type.is_dir() {
        return;
    }

    // Check excluded directory names
    if let Some(file_name) = current.file_name() {
        let name_str = file_name.to_string_lossy();
        if config.excluded_dir_names.iter().any(|ex| ex == &name_str) {
            return;
        }
        if config.ignore_hidden && name_str.starts_with('.') && current_depth > 0 {
            return;
        }
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                *coverage_status = coverage_status.merge(CoverageStatus::Degraded);
                diagnostics.push(ScanDiagnostic::permission_denied(
                    current.to_path_buf(),
                    scanner_name,
                    format!("Permission denied reading directory entries: {}", e),
                ));
            } else {
                diagnostics.push(ScanDiagnostic::io_error(
                    Some(current.to_path_buf()),
                    scanner_name,
                    e.to_string(),
                ));
            }
            return;
        }
    };

    for entry_res in entries {
        match entry_res {
            Ok(entry) => {
                walk_recursive(
                    &entry.path(),
                    current_depth + 1,
                    config,
                    scanner_name,
                    paths,
                    diagnostics,
                    coverage_status,
                );
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    *coverage_status = coverage_status.merge(CoverageStatus::Degraded);
                    diagnostics.push(ScanDiagnostic::permission_denied(
                        current.to_path_buf(),
                        scanner_name,
                        format!("Permission denied reading directory entry: {}", e),
                    ));
                } else {
                    diagnostics.push(ScanDiagnostic::io_error(
                        Some(current.to_path_buf()),
                        scanner_name,
                        e.to_string(),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_project_marker_detection() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let markers = detect_project_markers(dir.path());
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().any(|m| m.marker_type == ProjectMarkerType::Rust));
        assert!(markers.iter().any(|m| m.marker_type == ProjectMarkerType::Git));
        assert!(is_git_repository(dir.path()));
    }

    #[test]
    fn test_find_project_root() {
        let root = tempdir().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), "[package]").unwrap();

        let sub_dir = root.path().join("src").join("nested");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let found = find_project_root(&sub_dir, &["Cargo.toml"]);
        assert_eq!(found, Some(root.path().to_path_buf()));
    }

    #[test]
    fn test_walk_directory_safely_no_symlink_following() {
        let dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();

        let target_file = target_dir.path().join("secret.txt");
        std::fs::write(&target_file, b"secret data").unwrap();

        let symlink_path = dir.path().join("symlink_dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_dir.path(), &symlink_path).unwrap();

        let config = TraversalConfig::default();
        let result = walk_directory_safely(dir.path(), &config, "test_scanner");

        // The target_file inside target_dir should NOT be traversed
        assert!(!result.paths.iter().any(|p| p.ends_with("secret.txt")));
        assert_eq!(result.coverage_status, CoverageStatus::Complete);
    }
}
