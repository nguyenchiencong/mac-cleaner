use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::model::candidate::AllocatedSize;
use crate::model::diagnostic::ScanDiagnostic;

#[derive(Error, Debug)]
pub enum SizeError {
    #[error("Path not found: {0}")]
    NotFound(PathBuf),

    #[error("Permission denied calculating size for path: {0}")]
    PermissionDenied(PathBuf),

    #[error("I/O error calculating size: {0}")]
    Io(#[from] std::io::Error),
}

/// Calculates both logical and APFS-allocated storage space for a given path.
///
/// Safety & APFS invariants (ADR 0004):
/// - Never follows symlinks (uses `symlink_metadata`).
/// - Computes physical allocation using `st_blocks * 512`.
/// - Deduplicates hardlinks using `(dev, ino)` sets to prevent double-counting.
pub fn calculate_path_size(path: &Path) -> Result<AllocatedSize, SizeError> {
    let mut seen_inodes = HashSet::new();
    let mut diagnostics = Vec::new();
    calculate_path_size_internal(path, &mut seen_inodes, &mut diagnostics, "")
}

/// Calculates storage size while collecting non-fatal diagnostics (e.g. unreadable sub-files).
pub fn calculate_path_size_with_diagnostics(
    path: &Path,
    diagnostics: &mut Vec<ScanDiagnostic>,
    scanner_name: &str,
) -> Result<AllocatedSize, SizeError> {
    let mut seen_inodes = HashSet::new();
    calculate_path_size_internal(path, &mut seen_inodes, diagnostics, scanner_name)
}

fn calculate_path_size_internal(
    path: &Path,
    seen_inodes: &mut HashSet<(u64, u64)>,
    diagnostics: &mut Vec<ScanDiagnostic>,
    scanner_name: &str,
) -> Result<AllocatedSize, SizeError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Err(SizeError::NotFound(path.to_path_buf()));
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                return Err(SizeError::PermissionDenied(path.to_path_buf()));
            } else {
                return Err(SizeError::Io(e));
            }
        }
    };

    let file_type = meta.file_type();

    // If it's a symlink, never follow it; count symlink file size itself
    if file_type.is_symlink() {
        return Ok(extract_single_item_size(&meta, seen_inodes));
    }

    if !file_type.is_dir() {
        // Regular file or special device
        return Ok(extract_single_item_size(&meta, seen_inodes));
    }

    // Directory traversal
    let mut total_size = extract_single_item_size(&meta, seen_inodes);

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                diagnostics.push(ScanDiagnostic::permission_denied(
                    path.to_path_buf(),
                    scanner_name,
                    format!("Cannot read directory entries: {}", e),
                ));
                return Ok(total_size);
            }
            return Err(SizeError::Io(e));
        }
    };

    for entry_res in entries {
        let entry = match entry_res {
            Ok(ent) => ent,
            Err(e) => {
                diagnostics.push(ScanDiagnostic::io_error(
                    Some(path.to_path_buf()),
                    scanner_name,
                    format!("Error reading directory entry: {}", e),
                ));
                continue;
            }
        };

        let entry_path = entry.path();
        match calculate_path_size_internal(&entry_path, seen_inodes, diagnostics, scanner_name) {
            Ok(child_size) => {
                total_size += child_size;
            }
            Err(SizeError::PermissionDenied(p)) => {
                diagnostics.push(ScanDiagnostic::permission_denied(
                    p,
                    scanner_name,
                    "Permission denied reading path",
                ));
            }
            Err(SizeError::NotFound(p)) => {
                diagnostics.push(ScanDiagnostic::path_vanished(
                    p,
                    scanner_name,
                    "Path vanished during directory traversal",
                ));
            }
            Err(SizeError::Io(e)) => {
                diagnostics.push(ScanDiagnostic::io_error(
                    Some(entry_path),
                    scanner_name,
                    format!("I/O error during sizing: {}", e),
                ));
            }
        }
    }

    Ok(total_size)
}

fn extract_single_item_size(
    meta: &std::fs::Metadata,
    seen_inodes: &mut HashSet<(u64, u64)>,
) -> AllocatedSize {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = meta.dev();
        let ino = meta.ino();
        let nlink = meta.nlink();

        // Hardlink deduplication: if nlink > 1 and already seen, do not double-count
        if nlink > 1 {
            if !seen_inodes.insert((dev, ino)) {
                // Already counted this inode
                return AllocatedSize::ZERO;
            }
        } else {
            seen_inodes.insert((dev, ino));
        }

        let logical_bytes = if meta.is_dir() { 0 } else { meta.size() };
        let allocated_bytes = meta.blocks().saturating_mul(512);

        AllocatedSize::new(logical_bytes, allocated_bytes)
    }

    #[cfg(not(unix))]
    {
        let len = if meta.is_dir() { 0 } else { meta.len() };
        AllocatedSize::new(len, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_calculate_file_size() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        std::fs::write(&file, vec![0u8; 1024]).unwrap();

        let size = calculate_path_size(&file).unwrap();
        assert_eq!(size.logical_bytes, 1024);
        assert!(size.allocated_bytes > 0);
    }

    #[test]
    fn test_calculate_directory_size_and_hardlink_deduplication() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("original.bin");
        let file2 = dir.path().join("hardlink.bin");

        // Write 4KB
        std::fs::write(&file1, vec![0xAA; 4096]).unwrap();

        // Create hardlink
        std::fs::hard_link(&file1, &file2).unwrap();

        // When measuring the directory containing both hardlinks, size should only count the 4KB once!
        let dir_size = calculate_path_size(dir.path()).unwrap();
        assert_eq!(dir_size.logical_bytes, 4096);
    }

    #[test]
    fn test_symlink_is_not_followed() {
        let dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();

        // Write 10MB to target_dir
        let big_file = target_dir.path().join("large.bin");
        std::fs::write(&big_file, vec![0u8; 1024 * 1024]).unwrap();

        // Symlink inside dir pointing to target_dir
        let symlink_path = dir.path().join("link_to_big");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_dir.path(), &symlink_path).unwrap();

        let size = calculate_path_size(dir.path()).unwrap();
        // The symlink must NOT follow into target_dir
        assert!(size.logical_bytes < 1024 * 100);
    }

    #[test]
    fn test_not_found_error() {
        let res = calculate_path_size(Path::new("/non/existent/path/for/sure/12345"));
        assert!(matches!(res, Err(SizeError::NotFound(_))));
    }
}
