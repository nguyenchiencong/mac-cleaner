use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Available-space measurements for each filesystem containing an approved action.
///
/// The delta is an observation of volume-wide free space, not proof that the action
/// itself freed those bytes; concurrent filesystem activity and APFS snapshots can
/// affect the result.
#[derive(Debug, Default)]
pub(crate) struct FreeSpaceSnapshot {
    volumes: BTreeMap<u64, (PathBuf, u64)>,
}

impl FreeSpaceSnapshot {
    pub(crate) fn capture<'a>(paths: impl IntoIterator<Item = (&'a Path, u64)>) -> Self {
        let mut volumes = BTreeMap::new();

        for (path, device) in paths {
            if volumes.contains_key(&device) {
                continue;
            }

            let Some(anchor) = existing_path_on_device(path, device) else {
                continue;
            };
            let Some(available) = available_bytes(&anchor) else {
                continue;
            };
            volumes.insert(device, (anchor, available));
        }

        Self { volumes }
    }

    /// Returns the observed net change in available bytes across the measured volumes.
    /// A positive value means more space was available after execution.
    pub(crate) fn net_delta_bytes(&self) -> Option<i64> {
        if self.volumes.is_empty() {
            return None;
        }

        let mut before = 0u128;
        let mut after = 0u128;

        for (device, (anchor, before_bytes)) in &self.volumes {
            let anchor = existing_path_on_device(anchor, *device)?;
            let after_bytes = available_bytes(&anchor)?;
            before = before.saturating_add(u128::from(*before_bytes));
            after = after.saturating_add(u128::from(after_bytes));
        }

        Some(signed_delta_i64(before, after))
    }
}

fn existing_path_on_device(path: &Path, device: u64) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let metadata = std::fs::symlink_metadata(ancestor).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev() == device).then(|| ancestor.to_path_buf())
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            let _ = device;
            Some(ancestor.to_path_buf())
        }
    })
}

#[cfg(unix)]
fn available_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is a valid, NUL-terminated path and `stats` is writable.
    unsafe {
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) != 0 {
            return None;
        }
        let stats = stats.assume_init();
        let block_size = to_u64(if stats.f_frsize == 0 {
            stats.f_bsize
        } else {
            stats.f_frsize
        })?;
        let available_blocks = to_u64(stats.f_bavail)?;
        Some(available_blocks.saturating_mul(block_size))
    }
}

#[cfg(not(unix))]
fn available_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(unix)]
fn to_u64<T>(value: T) -> Option<u64>
where
    u64: TryFrom<T>,
{
    u64::try_from(value).ok()
}

fn signed_delta_i64(before: u128, after: u128) -> i64 {
    if after >= before {
        i64::try_from(after - before).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(before - after).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn signed_delta_reports_increases_and_decreases() {
        assert_eq!(signed_delta_i64(10, 18), 8);
        assert_eq!(signed_delta_i64(18, 10), -8);
        assert_eq!(signed_delta_i64(0, u128::MAX), i64::MAX);
        assert_eq!(signed_delta_i64(u128::MAX, 0), -i64::MAX);
    }

    #[cfg(unix)]
    #[test]
    fn captures_available_space_for_existing_path() {
        let directory = tempdir().unwrap();
        let device = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(directory.path()).unwrap().dev()
        };
        let snapshot = FreeSpaceSnapshot::capture([(directory.path(), device)]);
        assert_eq!(snapshot.volumes.len(), 1);
        assert!(snapshot.net_delta_bytes().is_some());
    }
}
