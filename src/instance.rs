//! Keep one serving process per data directory so library scans cannot conflict.
use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    path::Path,
};

/// The operating system releases the lock when this handle is dropped or the process exits.
#[derive(Debug)]
pub struct ServerInstance {
    _file: File,
}

impl Drop for ServerInstance {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

impl ServerInstance {
    pub fn acquire(data_dir: &Path) -> io::Result<Self> {
        let path = data_dir.join("server.lock");
        // Keep this file across restarts. Unlinking it could let another process lock
        // a new inode while a server still holds the old file's lock.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "Another server is already using data directory {} (lock: {})",
                    data_dir.display(),
                    path.display()
                ),
            )),
            Err(TryLockError::Error(error)) => Err(io::Error::new(
                error.kind(),
                format!("Cannot lock {}: {error}", path.display()),
            )),
        }
    }
}
