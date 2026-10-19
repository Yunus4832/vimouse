use crate::engine::Status;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub fn default_directory() -> PathBuf {
    // Use the actual user, never an inherited path when running as root.
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/vimouse-{uid}"))
}

pub struct Runtime {
    pub directory: PathBuf,
    lock: File,
    last: Vec<u8>,
}

pub fn prepare_directory(directory: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runtime directory must be a real directory owned by this user, with permissions 0700",
        ));
    }
    Ok(())
}

use std::os::unix::fs::DirBuilderExt;

impl Runtime {
    pub fn acquire(directory: PathBuf) -> io::Result<Self> {
        prepare_directory(&directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("daemon.lock"))?;
        // SAFETY: lock owns a valid fd and flock does not retain a pointer.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::other(
                "another Vimouse process is using this runtime directory",
            ));
        }
        Ok(Self {
            directory,
            lock,
            last: Vec::new(),
        })
    }

    pub fn write(&mut self, status: &Status) -> io::Result<()> {
        let data = serde_json::to_vec(status)?;
        if data == self.last {
            return Ok(());
        }
        let temporary = self.directory.join("status.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&data)?;
        fs::rename(temporary, self.directory.join("status.json"))?;
        self.last = data;
        Ok(())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.directory.join("status.json"));
        // Explicitly unlock before close: a concurrent fork may briefly retain
        // the open file description until exec applies O_CLOEXEC.
        // SAFETY: self.lock is a live fd; flock does not retain a pointer.
        unsafe {
            libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
