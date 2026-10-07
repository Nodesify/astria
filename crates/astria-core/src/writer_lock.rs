//! One OS-backed writer per project; readers never acquire this lock.
use crate::Result;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub struct WriterLock {
    _file: File,
}
impl WriterLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        Self::acquire_in(&root.join(".astria"))
    }
    /// Global stores use their containing directory as the lock namespace.
    pub fn acquire_in(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("writer.lock"))?;
        // Kernel locks release on process termination, including crashes.
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}

/// A unique sibling avoids collisions; persist atomically replaces the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
