//! Paths and process-group leaders a second Ctrl-C must clean up.
//!
//! `std::process::exit` skips `Drop`, so [`crate::queue`]'s temp-audio guard
//! and a demucs scratch directory would otherwise survive, and a child that
//! was put in its own process group would keep running. Registration holds
//! the registry lock only for the insert or remove. [`try_emergency_stop`]
//! uses `try_lock`: if the lock is busy it returns immediately and the
//! caller exits without waiting.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Group {
    id: u64,
    pid: u32,
}

struct Temp {
    id: u64,
    path: PathBuf,
    /// When set, [`try_emergency_stop`] leaves the file in place.
    /// Used for a denoised file the user asked to keep.
    keep: bool,
    directory: bool,
}

struct Registry {
    next_id: u64,
    groups: Vec<Group>,
    temps: Vec<Temp>,
}

impl Registry {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    next_id: 1,
    groups: Vec::new(),
    temps: Vec::new(),
});

/// Drops the process-group registration. Does not signal or wait.
///
/// Drop this only after the child has been `wait`ed, so a reused pid is
/// not still in the registry.
pub struct TrackedProcess {
    id: u64,
}

impl Drop for TrackedProcess {
    fn drop(&mut self) {
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.groups.retain(|group| group.id != self.id);
        }
    }
}

/// Drops the temp-path registration. Does not delete the path.
pub struct TrackedTemp {
    id: u64,
}

impl Drop for TrackedTemp {
    fn drop(&mut self) {
        if let Ok(mut registry) = REGISTRY.lock() {
            registry.temps.retain(|temp| temp.id != self.id);
        }
    }
}

/// Remember `pid` as a process-group leader (`process_group(0)` makes the
/// child's pid the group id). The lock is not held after this returns.
pub fn track_process(pid: u32) -> TrackedProcess {
    let mut registry = REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = registry.alloc_id();
    registry.groups.push(Group { id, pid });
    TrackedProcess { id }
}

/// Remember a file to delete on emergency exit unless `keep` is set.
///
/// Do not pass the user's source media. A denoised file the user asked to
/// keep uses `keep = true`.
pub fn track_temp_file(path: impl Into<PathBuf>, keep: bool) -> TrackedTemp {
    track_temp(path.into(), keep, false)
}

/// Remember a directory to delete on emergency exit (demucs scratch).
pub fn track_temp_dir(path: impl Into<PathBuf>) -> TrackedTemp {
    track_temp(path.into(), false, true)
}

fn track_temp(path: PathBuf, keep: bool, directory: bool) -> TrackedTemp {
    let mut registry = REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = registry.alloc_id();
    registry.temps.push(Temp {
        id,
        path,
        keep,
        directory,
    });
    TrackedTemp { id }
}

/// Kill registered groups, then delete registered temps.
///
/// `kill` is invoked while the registry lock is held and must not wait.
/// Returns `false` immediately when the lock is busy; the caller exits
/// without cleanup instead of blocking. A temp registered with `keep` is
/// left in place. Other temps are removed. Directory entries use
/// `remove_dir_all`. This is not unix-only; signaling inside `kill` is.
pub fn try_emergency_stop(kill: impl FnOnce(&[u32])) -> bool {
    let Ok(registry) = REGISTRY.try_lock() else {
        return false;
    };
    let pids: Vec<u32> = registry.groups.iter().map(|group| group.pid).collect();
    kill(&pids);
    for temp in &registry.temps {
        if temp.keep {
            continue;
        }
        remove_temp(&temp.path, temp.directory);
    }
    true
}

fn remove_temp(path: &Path, directory: bool) {
    let result = if directory {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    if let Err(err) = result
        && err.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("failed to remove temp path {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn emergency_stop_does_not_wait_when_the_registry_lock_is_held() {
        let _hold = REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let start = Instant::now();
        let ran = try_emergency_stop(|_| panic!("kill must not run while the lock is busy"));
        assert!(!ran);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(200),
            "try_lock waited {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn emergency_stop_deletes_speech_and_scratch_but_keeps_requested_denoise_and_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let speech = dir.path().join("clip_speech.mp3");
        let scratch = dir.path().join("scratch");
        let denoised = dir.path().join("clip_DeepFilterNet3.wav");
        let source = dir.path().join("clip.mp3");
        std::fs::write(&speech, b"speech").expect("speech");
        std::fs::create_dir(&scratch).expect("scratch");
        std::fs::write(scratch.join("partial"), b"partial").expect("partial");
        std::fs::write(&denoised, b"denoised").expect("denoised");
        std::fs::write(&source, b"source").expect("source");

        let _speech_track = track_temp_file(&speech, false);
        let _scratch_track = track_temp_dir(&scratch);
        let _denoised_track = track_temp_file(&denoised, true);

        assert!(try_emergency_stop(|_| {}));
        assert!(!speech.exists(), "speech temp must be removed");
        assert!(!scratch.exists(), "scratch dir must be removed");
        assert!(
            denoised.exists(),
            "denoised file must stay when keep_denoised_file is set"
        );
        assert!(source.exists(), "the user's source path is not registered");
    }

    #[test]
    fn emergency_stop_deletes_denoised_file_when_not_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let denoised = dir.path().join("clip_DeepFilterNet3.wav");
        std::fs::write(&denoised, b"denoised").expect("denoised");
        let _track = track_temp_file(&denoised, false);
        assert!(try_emergency_stop(|_| {}));
        assert!(!denoised.exists());
    }

    #[test]
    fn dropping_a_temp_registration_leaves_the_file_for_normal_cleanup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let speech = dir.path().join("clip_speech.mp3");
        std::fs::write(&speech, b"speech").expect("speech");
        {
            let _track = track_temp_file(&speech, false);
        }
        assert!(try_emergency_stop(|_| {}));
        assert!(speech.exists());
    }
}
