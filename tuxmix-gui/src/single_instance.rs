//! Hold an OS file lock for the lifetime of the GUI process.
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

pub fn acquire() -> io::Result<Option<File>> {
    let directory = if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime).join("tuxmix")
    } else {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        std::path::PathBuf::from(home).join(".cache/tuxmix")
    };
    fs::create_dir_all(&directory)?;
    lock(&directory.join("gui.lock"))
}

fn lock(path: &Path) -> io::Result<Option<File>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
    // Keep the file on disk: deleting it would let another process lock a
    // different file at the same path. The OS releases the lock on exit.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_second_instance_and_allows_restart() {
        let directory = std::env::temp_dir().join(format!("tuxmix-lock-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("gui.lock");
        let first = lock(&path).unwrap().expect("first instance acquires lock");
        assert!(lock(&path).unwrap().is_none());
        drop(first);
        let restarted = lock(&path).unwrap().expect("exit releases lock");
        drop(restarted);
        fs::remove_dir_all(directory).unwrap();
    }
}
