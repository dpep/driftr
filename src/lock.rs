//! One advisory lock per repo (shared by its worktrees), held across stage + commit and gc.
//!
//! Without it, concurrent sessions collide on the private index's `index.lock`, lose the
//! ref compare-and-swap, or — worse — a slow writer commits an older tree on top of a newer
//! one. Re-entrant within a process, so nested calls (restore's safety snapshot) don't deadlock.

use std::cell::{Cell, RefCell};
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::profile;

pub struct RepoLock {
    path: PathBuf,
    wait: Cell<Duration>,
    held: RefCell<Option<File>>,
    depth: Cell<u32>,
}

/// Releases the lock when the outermost guard drops.
pub struct Guard<'a>(&'a RepoLock);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let depth = self.0.depth.get() - 1;
        self.0.depth.set(depth);
        if depth == 0 {
            // Closing the file releases the lock.
            self.0.held.borrow_mut().take();
        }
    }
}

impl RepoLock {
    pub fn new(path: PathBuf) -> RepoLock {
        RepoLock {
            path,
            wait: Cell::new(Duration::from_secs(30)),
            held: RefCell::new(None),
            depth: Cell::new(0),
        }
    }

    /// How long `acquire` waits for another process before giving up.
    pub fn set_wait(&self, wait: Duration) {
        self.wait.set(wait);
    }

    pub fn acquire(&self) -> Result<Guard<'_>> {
        if self.depth.get() == 0 {
            let file = profile::time("lock wait", || self.open_locked())?;
            *self.held.borrow_mut() = Some(file);
        }
        self.depth.set(self.depth.get() + 1);
        Ok(Guard(self))
    }

    fn open_locked(&self) -> Result<File> {
        fs::create_dir_all(self.path.parent().unwrap())?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)?;
        let deadline = Instant::now() + self.wait.get();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(fs::TryLockError::WouldBlock) => {
                    bail!("busy: another driftr process holds {}", self.path.display())
                }
                Err(fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}
