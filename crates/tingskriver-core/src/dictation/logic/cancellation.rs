//! Cancellation ends at the insertion boundary, before any clipboard or history writes.

use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
struct State {
    generation: u64,
    active: usize,
}

#[derive(Default)]
pub struct Cancellation(Mutex<State>, tokio::sync::Notify);

impl Cancellation {
    pub const fn new() -> Self {
        Self(
            Mutex::new(State {
                generation: 0,
                active: 0,
            }),
            tokio::sync::Notify::const_new(),
        )
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn begin(&self) -> Pending<'_> {
        let mut state = self.state();
        state.active += 1;
        Pending {
            owner: self,
            generation: state.generation,
            active: Mutex::new(true),
        }
    }

    /// False means Escape belongs to the foreground application.
    pub fn cancel(&self) -> bool {
        let mut state = self.state();
        if state.active == 0 {
            return false;
        }
        state.generation += 1;
        self.1.notify_waiters();
        true
    }
}

pub struct Pending<'a> {
    owner: &'a Cancellation,
    generation: u64,
    active: Mutex<bool>,
}

impl Pending<'_> {
    pub async fn cancelled(&self) {
        loop {
            let notified = self.owner.1.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    pub fn is_cancelled(&self) -> bool {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        *active && self.owner.state().generation != self.generation
    }

    /// Atomically leave processing and decide whether insertion may begin.
    pub fn commit(&self) -> bool {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.owner.state();
        if !*active {
            return false;
        }
        *active = false;
        state.active -= 1;
        state.generation == self.generation
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let _ = self.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_escape_wakes_all_pending_turns_without_polling() {
        let cancellation = Cancellation::new();
        let first = cancellation.begin();
        let second = cancellation.begin();
        let wait = async {
            tokio::join!(first.cancelled(), second.cancelled());
        };
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut wait)
                .await
                .is_err()
        );
        assert!(cancellation.cancel());
        tokio::time::timeout(std::time::Duration::from_secs(1), wait)
            .await
            .unwrap();
        // Cancellation before subscribing must also wake immediately.
        tokio::time::timeout(std::time::Duration::from_secs(1), first.cancelled())
            .await
            .unwrap();
    }

    #[test]
    fn escape_outside_dictation_passes_through() {
        let cancellation = Cancellation::new();
        assert!(!cancellation.cancel());
        let pending = cancellation.begin();
        drop(pending);
        assert!(!cancellation.cancel());
    }

    #[test]
    fn cancelled_recording_or_processing_cannot_write_clipboard_or_history() {
        let cancellation = Cancellation::new();
        let recording = cancellation.begin();
        let processing = cancellation.begin();
        assert!(cancellation.cancel());
        let mut clipboard = "original";
        let mut history = Vec::new();
        for pending in [recording, processing] {
            assert!(pending.is_cancelled());
            if pending.commit() {
                clipboard = "dictation";
                history.push("dictation");
            }
        }
        assert_eq!(clipboard, "original");
        assert!(history.is_empty());
        assert!(!cancellation.cancel());
    }

    #[test]
    fn later_dictation_survives_previous_cancellation() {
        let cancellation = Cancellation::new();
        let old = cancellation.begin();
        assert!(cancellation.cancel());
        let new = cancellation.begin();
        assert!(!old.commit());
        assert!(new.commit());
        assert!(!new.commit());
    }

    #[test]
    fn concurrent_escape_and_commit_have_one_winner() {
        for _ in 0..100 {
            let cancellation = Cancellation::new();
            let pending = cancellation.begin();
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                let escape = scope.spawn(|| {
                    barrier.wait();
                    cancellation.cancel()
                });
                barrier.wait();
                let committed = pending.commit();
                assert_ne!(escape.join().unwrap(), committed);
            });
        }
    }

    #[test]
    fn insertion_is_no_longer_cancellable() {
        let cancellation = Cancellation::new();
        let inserted = cancellation.begin();
        assert!(inserted.commit());
        assert!(!cancellation.cancel());
        let next = cancellation.begin();
        assert!(cancellation.cancel());
        assert!(!inserted.is_cancelled());
        assert!(next.is_cancelled());
    }
}
