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

impl TrackedProcess {
    /// Registry id for [`try_emergency_stop_registered`]. Not a process id.
    pub fn registration_id(&self) -> u64 {
        self.id
    }
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

impl TrackedTemp {
    /// Registry id for [`try_emergency_stop_registered`]. Not a process id.
    pub fn registration_id(&self) -> u64 {
        self.id
    }
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
    stop_locked(&registry, |_| true, |_| true, kill);
    true
}

/// Same as [`try_emergency_stop`], but only the given registrations.
///
/// A lib test can stop the child and temps it registered without signaling
/// or deleting another test's live entries in this process. The real
/// handler still calls [`try_emergency_stop`] and covers every registration.
pub fn try_emergency_stop_registered(
    groups: &[u64],
    temps: &[u64],
    kill: impl FnOnce(&[u32]),
) -> bool {
    let Ok(registry) = REGISTRY.try_lock() else {
        return false;
    };
    stop_locked(
        &registry,
        |group| groups.contains(&group.id),
        |temp| temps.contains(&temp.id),
        kill,
    );
    true
}

fn stop_locked(
    registry: &Registry,
    groups: impl Fn(&Group) -> bool,
    temps: impl Fn(&Temp) -> bool,
    kill: impl FnOnce(&[u32]),
) {
    let pids: Vec<u32> = registry
        .groups
        .iter()
        .filter(|group| groups(group))
        .map(|group| group.pid)
        .collect();
    kill(&pids);
    for temp in &registry.temps {
        if !temps(temp) || temp.keep {
            continue;
        }
        remove_temp(&temp.path, temp.directory);
    }
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

    /// Stops only `temps`.
    ///
    /// [`emergency_stop_does_not_wait_when_the_registry_lock_is_held`] holds
    /// the registry lock on another thread, and [`try_emergency_stop_registered`]
    /// then returns false without deleting. Retry until this caller's own
    /// registrations are stopped. Never call [`try_emergency_stop`]: that
    /// deletes every temp still registered in the process, including a file
    /// another test has not dropped yet.
    fn stop_only(temps: &[u64]) {
        let start = Instant::now();
        while !try_emergency_stop_registered(&[], temps, |_| {}) {
            assert!(
                start.elapsed() < std::time::Duration::from_secs(2),
                "registry lock stayed busy"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
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

        let speech_track = track_temp_file(&speech, false);
        let scratch_track = track_temp_dir(&scratch);
        let denoised_track = track_temp_file(&denoised, true);

        stop_only(&[
            speech_track.registration_id(),
            scratch_track.registration_id(),
            denoised_track.registration_id(),
        ]);
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
        let track = track_temp_file(&denoised, false);
        // Only this registration. A process-global stop would also delete a
        // file another lib test still has registered.
        stop_only(&[track.registration_id()]);
        assert!(!denoised.exists());
    }

    #[test]
    fn dropping_a_temp_registration_leaves_the_file_for_normal_cleanup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let speech = dir.path().join("clip_speech.mp3");
        std::fs::write(&speech, b"speech").expect("speech");
        let id = {
            let track = track_temp_file(&speech, false);
            track.registration_id()
        };
        // The registration is gone. Stopping that id must not delete the
        // file, and must not sweep temps other tests still have registered.
        stop_only(&[id]);
        assert!(speech.exists());
    }
}
