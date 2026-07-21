//! Single-instance coordination via advisory file locks in `paths::data_dir()`
//! (ADR-017).
//!
//! ## The problem this closes (`docs/HUMM_SURFACE.md` §7)
//!
//! A crashed/killed humm process can leave `queued`/`in_progress` rows in the
//! store, so every process flips those rows to `canceled` at startup
//! (`SqliteStore::cancel_stale_tasks`) — otherwise a crash leaves a phantom
//! "running" task forever. But if a *second* humm process starts while the
//! first is still genuinely running, the second one's startup flip can race
//! the first one's live `in_progress` row and cancel it out from under it.
//! This module closes that race without forbidding concurrent `humm add`:
//! only the process that can prove it is the *sole* live humm process runs
//! the flip; every other process skips it and trusts that whatever it sees
//! as `in_progress` might be somebody else's real work.
//!
//! ## Protocol
//!
//! `instance.lock` (`data_dir/instance.lock`): every humm process, at
//! startup, attempts a non-blocking *exclusive* claim.
//!
//! - Exclusive claim succeeds ⇒ no other humm process currently holds any
//!   claim on the file, i.e. this process is the sole live instance, so it is
//!   safe to run `cancel_stale_tasks()` (every stale row it sees really is
//!   stale). It then *downgrades* to a shared claim, held for the rest of the
//!   process's life. Downgrading (rather than keeping the exclusive claim
//!   forever) is what lets a *second* `humm add` start moments later and run
//!   concurrently — humm never serializes `add` invocations, only the
//!   stale-flip decision.
//! - Exclusive claim fails (another process already holds a claim, shared or
//!   exclusive) ⇒ this process skips the flip entirely and takes a shared
//!   claim of its own instead (shared claims never contend with each other,
//!   so any number of `humm add` invocations can run side by side).
//!
//! `gui.lock` (`data_dir/gui.lock`) is a *separate* file used only for GUI
//! exclusivity (wired up in a later M3 task): the GUI takes an exclusive
//! claim and never downgrades it, so a second GUI launch's attempt fails and
//! it can show "humm is already running" instead of racing the first GUI
//! over shared UI state. It is independent of `instance.lock` — GUI processes
//! still participate in the stale-flip protocol via `instance.lock` like any
//! other humm process.
//!
//! ## Why the lock is deliberately leaked
//!
//! [`fd_lock::RwLock::try_write`]/[`try_read`] borrow `&mut self`/`&self`, so
//! their guards borrow from the `RwLock` they came from. [`InstanceGuard`]
//! and [`InstanceLock`] need to be ordinary, non-generic types with no
//! lifetime parameter (every caller across two binaries needs to hold one for
//! the rest of `main`, without threading a lifetime through their own
//! signatures), so the guard they hold has to borrow `'static`. Rather than
//! reach for a self-referential-struct crate (`ouroboros`/`self_cell`) for a
//! problem this small, the `RwLock<File>` is `Box::leak`ed: the resulting
//! `&'static mut RwLock<File>` lets its guards borrow `'static` too, without
//! the struct ever owning the data it borrows from (the leaked allocation
//! lives outside it, so there is no self-reference to construct).
//!
//! This is not a leak in the "bug" sense: the entire point of this lock is to
//! live exactly as long as the process does, so the allocation would never be
//! freed anyway (it would just sit there, unreleased, until exit). `Box::leak`
//! merely makes that explicit and gives the type system a sound `'static`
//! borrow to hand out. Each of `acquire_instance_lock`/`acquire_gui_lock`
//! leaks at most one small allocation, once per process.
//!
//! ## The downgrade gap
//!
//! Between dropping the exclusive guard and re-acquiring a shared one on the
//! success path, this process briefly holds no lock at all. If another
//! process's own exclusive attempt lands in that exact instant, it will also
//! see itself as the sole instance and also run `cancel_stale_tasks()`. This
//! is benign: any row that predates *both* processes' startups gets flipped
//! (correctly) by whichever one gets there first; nothing this process
//! enqueues after its own successful claim can look stale yet (there hasn't
//! been time for it to run), so there is nothing genuinely-running for the
//! interloper to wrongly cancel. This isn't just a timing argument — it's
//! load-bearing on call-site *ordering*: both callers (`humm-cli`'s
//! `run.rs::execute` and `humm-gui`'s `services.rs::init`) call
//! [`acquire_instance_lock`] and run the stale flip strictly *before* that
//! same process's first `enqueue` call, so by construction no row this
//! process itself creates exists yet for an interloper to race.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use fd_lock::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock file name for the stale-flip protocol, under `paths::data_dir()`.
const INSTANCE_LOCK_FILE: &str = "instance.lock";
/// Lock file name for GUI exclusivity, under `paths::data_dir()`.
const GUI_LOCK_FILE: &str = "gui.lock";

/// A held advisory-lock claim, released when dropped.
///
/// Wraps either a shared or an exclusive guard on a leaked, process-lifetime
/// [`fd_lock::RwLock`] (see the module docs for why the leak is deliberate).
/// Holding this alive keeps the underlying `flock` held; dropping it releases
/// the claim immediately.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "held only for its Drop side effect: releasing the flock"
)]
pub struct InstanceGuard(Claim);

/// The claim an [`InstanceGuard`] holds. Kept as a private enum rather than
/// two separate guard types so both [`acquire_instance_lock`] (which ends up
/// holding shared) and [`acquire_gui_lock`] (which holds exclusive) can share
/// one public guard type.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "each variant is held only for its Drop side effect"
)]
enum Claim {
    Shared(RwLockReadGuard<'static, File>),
    Exclusive(RwLockWriteGuard<'static, File>),
}

/// The result of [`acquire_instance_lock`].
#[derive(Debug)]
pub struct InstanceLock {
    /// Held for the process lifetime; dropping this releases the underlying
    /// `flock` claim on `instance.lock`.
    pub guard: InstanceGuard,
    /// `true` when this process won the exclusive claim at startup, meaning
    /// no other humm process was alive at that moment and it is safe to run
    /// `cancel_stale_tasks`. `false` means another humm process already held
    /// a claim, so the stale-flip must be skipped (see module docs).
    pub sole_instance: bool,
}

/// Opens (creating if absent) `data_dir/instance.lock` and runs the
/// stale-flip protocol described in the module docs: a non-blocking
/// exclusive attempt decides `sole_instance`, and the returned
/// [`InstanceLock::guard`] always ends up holding a *shared* claim, held for
/// the process lifetime.
///
/// The only `Err` this can return is a genuine I/O failure opening or
/// creating the lock file (e.g. an unwritable `data_dir`) — lock
/// *contention* is never an error, it is exactly what produces
/// `sole_instance: false`. Callers must treat an `Err` as non-fatal: fall
/// back to flipping stale rows as before (a lock failure must never brick
/// the CLI or GUI).
pub fn acquire_instance_lock(data_dir: &Path) -> io::Result<InstanceLock> {
    let file = open_lock_file(data_dir, INSTANCE_LOCK_FILE)?;
    // See "Why the lock is deliberately leaked" above: this is the one
    // allocation this process ever leaks for this lock, by design.
    let lock: &'static mut RwLock<File> = Box::leak(Box::new(RwLock::new(file)));

    // `try_claim_and_release` fully contains the exclusive borrow of `*lock`
    // within its own call frame (it returns only a `bool`, nothing that
    // borrows from `lock`), so it is provably over by the time `lock.read()`
    // runs below — unlike matching `lock.try_write()` inline, which extends
    // that temporary's (and therefore its mutable borrow's) drop scope across
    // the whole enclosing `match`/function per Rust's scrutinee-temporary
    // rule and conflicts with the shared borrow that follows.
    let sole_instance = try_claim_and_release(lock);

    // Either branch ends up here holding a shared claim: the winner
    // downgrades from exclusive (see "the downgrade gap" above so subsequent
    // `humm add` invocations aren't blocked); the loser takes shared directly
    // (never indefinitely blocked — nothing in this protocol holds
    // `instance.lock` exclusively for longer than another process's own brief
    // startup flip).
    let read_guard = lock.read()?;
    Ok(InstanceLock {
        guard: InstanceGuard(Claim::Shared(read_guard)),
        sole_instance,
    })
}

/// Attempts a non-blocking exclusive claim on `lock`, releasing it
/// immediately if won. Returns `true` iff the claim was won — i.e. no other
/// process held any claim on the file at that instant.
fn try_claim_and_release(lock: &mut RwLock<File>) -> bool {
    match lock.try_write() {
        Ok(write_guard) => {
            drop(write_guard);
            true
        }
        Err(_) => false,
    }
}

/// Opens (creating if absent) `data_dir/gui.lock` and attempts a non-blocking
/// exclusive claim, held for the process lifetime if won (never downgraded —
/// this is GUI exclusivity, a different protocol from
/// [`acquire_instance_lock`]'s stale-flip one).
///
/// Returns `Ok(None)` when another GUI process already holds the claim (the
/// caller should show "humm is already running" and exit); `Ok(Some(_))` when
/// this process won it. Only a genuine I/O failure opening/creating the lock
/// file is `Err`.
pub fn acquire_gui_lock(data_dir: &Path) -> io::Result<Option<InstanceGuard>> {
    let file = open_lock_file(data_dir, GUI_LOCK_FILE)?;
    let lock: &'static mut RwLock<File> = Box::leak(Box::new(RwLock::new(file)));
    match lock.try_write() {
        Ok(write_guard) => Ok(Some(InstanceGuard(Claim::Exclusive(write_guard)))),
        Err(_) => Ok(None),
    }
}

/// Opens `data_dir/<name>` for reading and writing, creating both the
/// directory and the file if absent. Never truncates: the lock file's
/// content is irrelevant (only its `flock` state matters), and truncating on
/// every open would needlessly rewrite it each process start.
fn open_lock_file(data_dir: &Path, name: &str) -> io::Result<File> {
    std::fs::create_dir_all(data_dir)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data_dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_acquire_is_sole_instance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lock = acquire_instance_lock(dir.path()).expect("acquire");
        assert!(lock.sole_instance);
    }

    #[test]
    fn second_acquire_while_first_held_is_not_sole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_instance_lock(dir.path()).expect("first acquire");
        assert!(first.sole_instance);

        let second = acquire_instance_lock(dir.path()).expect("second acquire");
        assert!(!second.sole_instance);

        // Both guards are shared claims and coexist fine.
        drop(first);
        drop(second);
    }

    #[test]
    fn sole_again_after_first_released() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_instance_lock(dir.path()).expect("first acquire");
        assert!(first.sole_instance);
        drop(first);

        let second = acquire_instance_lock(dir.path()).expect("second acquire");
        assert!(second.sole_instance);
    }

    #[test]
    fn gui_lock_exclusive_second_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_gui_lock(dir.path()).expect("first acquire");
        assert!(first.is_some());

        let second = acquire_gui_lock(dir.path()).expect("second acquire");
        assert!(second.is_none());

        drop(first);
    }

    #[test]
    fn lock_files_are_created_under_data_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _instance = acquire_instance_lock(dir.path()).expect("acquire instance lock");
        let _gui = acquire_gui_lock(dir.path()).expect("acquire gui lock");

        assert!(dir.path().join(INSTANCE_LOCK_FILE).is_file());
        assert!(dir.path().join(GUI_LOCK_FILE).is_file());
    }
}
