//! Cooperative cancellation signal shared across threads.
//!
//! Long-running operations (decode, inference, download) poll a [`CancelToken`]
//! rather than being forcibly interrupted (`docs/design/ARCHITECTURE.md` §3).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cheap, cloneable flag used to request cooperative cancellation.
///
/// Every clone shares the same underlying flag: calling [`CancelToken::cancel`]
/// on any clone is visible to all others via [`CancelToken::is_canceled`].
#[derive(Debug, Clone)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Creates a fresh token that has not been canceled.
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    /// Requests cancellation. Visible to every clone of this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if [`CancelToken::cancel`] has been called on this token
    /// or any of its clones.
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_token_is_not_canceled() {
        let token = CancelToken::new();
        assert!(!token.is_canceled());
    }

    #[test]
    fn cancel_is_visible_on_all_clones() {
        let token = CancelToken::new();
        let clone_before = token.clone();
        token.cancel();
        let clone_after = token.clone();

        assert!(token.is_canceled());
        assert!(clone_before.is_canceled());
        assert!(clone_after.is_canceled());
    }
}
