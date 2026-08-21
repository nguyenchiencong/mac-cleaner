use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

use crate::model::candidate::RevalidationFingerprint;
use crate::model::manifest::TrashReceipt;

#[derive(Error, Debug)]
pub enum TrashError {
    #[error("Item not found at path: {0}")]
    NotFound(PathBuf),

    #[error("Permission denied accessing path: {0}")]
    PermissionDenied(PathBuf),

    #[error("Destination already exists: {0}")]
    DestinationAlreadyExists(PathBuf),

    #[error("Trash backend error: {0}")]
    BackendError(String),

    #[error("I/O error during trash operation: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum RestoreError {
    #[error("Item not found in Trash: {0}")]
    TrashItemNotFound(PathBuf),

    #[error("Destination collision: path already exists at {path}")]
    DestinationCollision { path: PathBuf },

    #[error("Permission denied restoring path: {0}")]
    PermissionDenied(PathBuf),

    #[error("Restore backend error: {0}")]
    BackendError(String),

    #[error("I/O error during restore operation: {0}")]
    Io(#[from] std::io::Error),
}

/// Abstracted interface for moving candidates to Trash and restoring them via receipts.
pub trait TrashBackend: Send + Sync {
    /// Relocates the candidate at `path` to Trash and returns an audit receipt.
    fn move_to_trash(&self, path: &Path) -> Result<TrashReceipt, TrashError>;

    /// Restores a previously trashed candidate to its original location using the receipt.
    fn restore(&self, receipt: &TrashReceipt) -> Result<(), RestoreError>;
}

/// Native macOS Trash backend utilizing NSFileManager and Cocoa APIs.
#[derive(Debug, Default, Clone)]
pub struct NativeTrashBackend;

impl NativeTrashBackend {
    pub fn new() -> Self {
        Self
    }
}

impl TrashBackend for NativeTrashBackend {
    fn move_to_trash(&self, path: &Path) -> Result<TrashReceipt, TrashError> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !path.exists() && std::fs::symlink_metadata(&path).is_err() {
            return Err(TrashError::NotFound(path));
        }

        // Capture fingerprint and size before moving
        let fingerprint = RevalidationFingerprint::from_path(&path)?;
        let size = crate::scan::size::calculate_path_size(&path).unwrap_or_default();

        #[cfg(target_os = "macos")]
        {
            let trash_path = macos_trash_item(&path).map_err(TrashError::BackendError)?;
            Ok(TrashReceipt::new(path, trash_path, fingerprint, size))
        }

        #[cfg(not(target_os = "macos"))]
        {
            trash::delete(&path).map_err(|e| TrashError::BackendError(e.to_string()))?;
            let fallback_trash_path = path.clone();
            Ok(TrashReceipt::new(path, fallback_trash_path, fingerprint, size))
        }
    }

    fn restore(&self, receipt: &TrashReceipt) -> Result<(), RestoreError> {
        if !receipt.trash_path.exists() && std::fs::symlink_metadata(&receipt.trash_path).is_err() {
            return Err(RestoreError::TrashItemNotFound(receipt.trash_path.clone()));
        }

        if receipt.original_path.exists() || std::fs::symlink_metadata(&receipt.original_path).is_ok() {
            return Err(RestoreError::DestinationCollision {
                path: receipt.original_path.clone(),
            });
        }

        if let Some(parent) = receipt.original_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Attempt direct atomic rename
        if let Err(_rename_err) = std::fs::rename(&receipt.trash_path, &receipt.original_path) {
            // Fallback for cross-device moves
            copy_and_remove(&receipt.trash_path, &receipt.original_path)
                .map_err(|e| RestoreError::BackendError(format!("Failed to restore cross-device: {}", e)))?;
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn macos_trash_item(path: &Path) -> Result<PathBuf, String> {
    use std::ffi::CString;
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};

    let path_str = path
        .to_str()
        .ok_or_else(|| "Path contains invalid UTF-8".to_string())?;
    let c_path = CString::new(path_str).map_err(|e| e.to_string())?;

    unsafe {
        let ns_string_class = Class::get("NSString").ok_or("NSString class not found")?;
        let ns_url_class = Class::get("NSURL").ok_or("NSURL class not found")?;
        let ns_file_manager_class =
            Class::get("NSFileManager").ok_or("NSFileManager class not found")?;

        let ns_path: *mut Object =
            msg_send![ns_string_class, stringWithUTF8String: c_path.as_ptr()];
        if ns_path.is_null() {
            return Err("Failed to create NSString for path".to_string());
        }

        let ns_url: *mut Object = msg_send![ns_url_class, fileURLWithPath: ns_path];
        if ns_url.is_null() {
            return Err("Failed to create NSURL for path".to_string());
        }

        let file_manager: *mut Object = msg_send![ns_file_manager_class, defaultManager];
        if file_manager.is_null() {
            return Err("Failed to get default NSFileManager".to_string());
        }

        let mut resulting_url: *mut Object = std::ptr::null_mut();
        let mut error: *mut Object = std::ptr::null_mut();

        let success: bool = msg_send![
            file_manager,
            trashItemAtURL: ns_url
            resultingItemURL: &mut resulting_url
            error: &mut error
        ];

        if !success {
            if !error.is_null() {
                let desc: *mut Object = msg_send![error, localizedDescription];
                if !desc.is_null() {
                    let utf8: *const std::os::raw::c_char = msg_send![desc, UTF8String];
                    if !utf8.is_null() {
                        let msg = std::ffi::CStr::from_ptr(utf8)
                            .to_string_lossy()
                            .into_owned();
                        return Err(format!("macOS trashItemAtURL failed: {}", msg));
                    }
                }
            }
            return Err("macOS trashItemAtURL returned false".to_string());
        }

        if !resulting_url.is_null() {
            let res_path: *mut Object = msg_send![resulting_url, path];
            if !res_path.is_null() {
                let utf8: *const std::os::raw::c_char = msg_send![res_path, UTF8String];
                if !utf8.is_null() {
                    let trash_path_str = std::ffi::CStr::from_ptr(utf8)
                        .to_string_lossy()
                        .into_owned();
                    return Ok(PathBuf::from(trash_path_str));
                }
            }
        }

        // Fallback if resulting_url was null but success was true
        if let Some(filename) = path.file_name() {
            if let Some(home) = std::env::var_os("HOME") {
                let trash_dest = PathBuf::from(home).join(".Trash").join(filename);
                return Ok(trash_dest);
            }
        }

        Ok(path.to_path_buf())
    }
}

/// Helper function to copy and remove a file or directory tree for cross-device moves.
fn copy_and_remove(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let entry_dst = dst.join(entry.file_name());
            copy_and_remove(&entry.path(), &entry_dst)?;
        }
        std::fs::remove_dir(src)?;
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}

/// In-memory and sandbox-backed Trash backend for deterministic testing and dry runs.
#[derive(Debug, Clone)]
pub struct FakeTrashBackend {
    trash_dir: PathBuf,
    injected_trash_errors: Arc<Mutex<HashMap<PathBuf, String>>>,
    injected_restore_errors: Arc<Mutex<HashMap<PathBuf, String>>>,
    trashed_receipts: Arc<Mutex<Vec<TrashReceipt>>>,
}

impl FakeTrashBackend {
    /// Creates a new fake trash backend rooted at the given directory.
    pub fn new(trash_dir: impl AsRef<Path>) -> Self {
        let trash_dir = trash_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&trash_dir).ok();
        Self {
            trash_dir,
            injected_trash_errors: Arc::new(Mutex::new(HashMap::new())),
            injected_restore_errors: Arc::new(Mutex::new(HashMap::new())),
            trashed_receipts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Injects a failure for a specific path during trash operations.
    pub fn inject_trash_error(&self, path: impl AsRef<Path>, error_msg: impl Into<String>) {
        self.injected_trash_errors
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), error_msg.into());
    }

    /// Injects a failure for a specific path during restore operations.
    pub fn inject_restore_error(&self, path: impl AsRef<Path>, error_msg: impl Into<String>) {
        self.injected_restore_errors
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), error_msg.into());
    }

    /// Returns all receipts recorded by this backend.
    pub fn recorded_receipts(&self) -> Vec<TrashReceipt> {
        self.trashed_receipts.lock().unwrap().clone()
    }
}

impl TrashBackend for FakeTrashBackend {
    fn move_to_trash(&self, path: &Path) -> Result<TrashReceipt, TrashError> {
        if let Some(err) = self.injected_trash_errors.lock().unwrap().get(path) {
            return Err(TrashError::BackendError(err.clone()));
        }

        if !path.exists() && std::fs::symlink_metadata(path).is_err() {
            return Err(TrashError::NotFound(path.to_path_buf()));
        }

        let fingerprint = RevalidationFingerprint::from_path(path)?;
        let size = crate::scan::size::calculate_path_size(path).unwrap_or_default();

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("item_{}", Uuid::new_v4()));

        // Generate unique destination in fake trash directory
        let mut target_trash_path = self.trash_dir.join(&file_name);
        let mut counter = 1;
        while target_trash_path.exists() || std::fs::symlink_metadata(&target_trash_path).is_ok() {
            target_trash_path = self.trash_dir.join(format!("{}.{}", file_name, counter));
            counter += 1;
        }

        copy_and_remove(path, &target_trash_path)?;

        let receipt = TrashReceipt::new(
            path.to_path_buf(),
            target_trash_path,
            fingerprint,
            size,
        );

        self.trashed_receipts.lock().unwrap().push(receipt.clone());
        Ok(receipt)
    }

    fn restore(&self, receipt: &TrashReceipt) -> Result<(), RestoreError> {
        if let Some(err) = self.injected_restore_errors.lock().unwrap().get(&receipt.trash_path) {
            return Err(RestoreError::BackendError(err.clone()));
        }

        if !receipt.trash_path.exists() && std::fs::symlink_metadata(&receipt.trash_path).is_err() {
            return Err(RestoreError::TrashItemNotFound(receipt.trash_path.clone()));
        }

        if receipt.original_path.exists() || std::fs::symlink_metadata(&receipt.original_path).is_ok() {
            return Err(RestoreError::DestinationCollision {
                path: receipt.original_path.clone(),
            });
        }

        if let Some(parent) = receipt.original_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        copy_and_remove(&receipt.trash_path, &receipt.original_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_fake_trash_and_restore_cycle() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let backend = FakeTrashBackend::new(trash_dir.path());

        let file_path = work_dir.path().join("target_file.txt");
        std::fs::write(&file_path, b"test payload for trash").unwrap();
        assert!(file_path.exists());

        // Move to trash
        let receipt = backend.move_to_trash(&file_path).unwrap();
        assert!(!file_path.exists());
        assert!(receipt.trash_path.exists());
        assert_eq!(receipt.original_path, file_path);

        // Restore
        backend.restore(&receipt).unwrap();
        assert!(file_path.exists());
        assert!(!receipt.trash_path.exists());
        assert_eq!(std::fs::read(&file_path).unwrap(), b"test payload for trash");
    }

    #[test]
    fn test_fake_trash_directory_cycle() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let backend = FakeTrashBackend::new(trash_dir.path());

        let dir_path = work_dir.path().join("build_dir");
        std::fs::create_dir_all(&dir_path).unwrap();
        std::fs::write(dir_path.join("a.o"), b"object a").unwrap();
        std::fs::write(dir_path.join("b.o"), b"object b").unwrap();

        let receipt = backend.move_to_trash(&dir_path).unwrap();
        assert!(!dir_path.exists());
        assert!(receipt.trash_path.exists());
        assert!(receipt.trash_path.join("a.o").exists());

        // Restore directory
        backend.restore(&receipt).unwrap();
        assert!(dir_path.exists());
        assert!(dir_path.join("a.o").exists());
        assert!(dir_path.join("b.o").exists());
    }

    #[test]
    fn test_fake_trash_destination_collision_prevention() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let backend = FakeTrashBackend::new(trash_dir.path());

        let file_path = work_dir.path().join("collide.txt");
        std::fs::write(&file_path, b"initial").unwrap();

        let receipt = backend.move_to_trash(&file_path).unwrap();

        // Create new file at original location before restore
        std::fs::write(&file_path, b"new colliding file").unwrap();

        // Restore must fail with DestinationCollision to avoid data loss
        let result = backend.restore(&receipt);
        assert!(matches!(result, Err(RestoreError::DestinationCollision { .. })));
    }

    #[test]
    fn test_fake_trash_fault_injection() {
        let work_dir = tempdir().unwrap();
        let trash_dir = tempdir().unwrap();
        let backend = FakeTrashBackend::new(trash_dir.path());

        let file_path = work_dir.path().join("fail_move.txt");
        std::fs::write(&file_path, b"data").unwrap();

        backend.inject_trash_error(&file_path, "Injected disk write failure");

        let err = backend.move_to_trash(&file_path).unwrap_err();
        assert!(matches!(err, TrashError::BackendError(msg) if msg.contains("Injected disk write failure")));
        assert!(file_path.exists());
    }
}
