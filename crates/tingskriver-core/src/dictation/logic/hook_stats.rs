//! How long the keyboard hook callback takes.
//!
//! Windows removes a `WH_KEYBOARD_LL` hook that is too slow, and every key
//! press in the system waits for it, so the callback must stay well under
//! 1 ms. The hook records the duration of every call here; the summary only
//! holds counts and times, never which key was pressed.
//!
//! The counters are atomics, so recording never blocks and needs no lock.

use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Durations up to this many microseconds get their own bucket; longer ones
/// share the last bucket (their exact value still counts for `max`).
pub const BUCKET_LIMIT_US: usize = 2_000;

/// A summary is due after this many calls.
pub const WINDOW: u64 = 10_000;

/// Call durations of the keyboard hook, in 1 µs buckets.
pub struct HookTimes {
    buckets: [AtomicU32; BUCKET_LIMIT_US + 1],
    count: AtomicU64,
    max_ns: AtomicU64,
}

impl HookTimes {
    pub const fn new() -> Self {
        HookTimes {
            buckets: [const { AtomicU32::new(0) }; BUCKET_LIMIT_US + 1],
            count: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
        }
    }

    /// Record one call. Returns true when [`WINDOW`] calls have been
    /// recorded since the last [`HookTimes::take`], so a summary is due.
    pub fn record(&self, elapsed_ns: u64) -> bool {
        let bucket = ((elapsed_ns / 1_000) as usize).min(BUCKET_LIMIT_US);
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
        self.max_ns.fetch_max(elapsed_ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed) + 1 == WINDOW
    }

    /// Number of calls since the last [`HookTimes::take`].
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Summary of the calls so far, without resetting.
    pub fn summary(&self) -> HookSummary {
        let counts: Vec<u32> = self.buckets.iter().map(|b| b.load(Ordering::Relaxed)).collect();
        HookSummary::from_buckets(&counts, self.max_ns.load(Ordering::Relaxed))
    }

    /// Summary of the calls so far, and start a new window.
    pub fn take(&self) -> HookSummary {
        let counts: Vec<u32> = self.buckets.iter().map(|b| b.swap(0, Ordering::Relaxed)).collect();
        let max_ns = self.max_ns.swap(0, Ordering::Relaxed);
        self.count.store(0, Ordering::Relaxed);
        HookSummary::from_buckets(&counts, max_ns)
    }
}

impl Default for HookTimes {
    fn default() -> Self {
        Self::new()
    }
}

/// Percentiles of the hook's call durations. Only numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HookSummary {
    pub count: u64,
    pub p50_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

impl HookSummary {
    fn from_buckets(counts: &[u32], max_ns: u64) -> HookSummary {
        let count: u64 = counts.iter().map(|&c| c as u64).sum();
        let max_us = max_ns.div_ceil(1_000);
        // A bucket holds durations from n µs up to (but not including) n+1 µs;
        // the percentile reports the bucket's upper edge so it never
        // understates, capped by the real maximum.
        let percentile = |p: u64| -> u64 {
            if count == 0 {
                return 0;
            }
            let rank = (count * p).div_ceil(100).max(1);
            let mut seen = 0u64;
            for (us, &c) in counts.iter().enumerate() {
                seen += c as u64;
                if seen >= rank {
                    return (us as u64 + 1).min(max_us.max(1));
                }
            }
            max_us
        };
        HookSummary {
            count,
            p50_us: percentile(50),
            p99_us: percentile(99),
            max_us,
        }
    }

    /// The hook met its target: p99 under 1 ms.
    pub fn within_target(&self) -> bool {
        self.p99_us < 1_000
    }
}

impl fmt::Display for HookSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} calls, p50 {} us, p99 {} us, max {} us",
            self.count, self.p50_us, self.p99_us, self.max_us
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_is_zero() {
        let times = HookTimes::new();
        assert_eq!(times.summary(), HookSummary::default());
        assert!(times.summary().within_target());
    }

    #[test]
    fn percentiles_follow_the_distribution() {
        let times = HookTimes::new();
        // 98 fast calls (3.5 µs), one at 40 µs and one slow one at 1.5 ms.
        for _ in 0..98 {
            times.record(3_500);
        }
        times.record(40_000);
        times.record(1_500_000);
        let s = times.summary();
        assert_eq!(s.count, 100);
        assert_eq!(s.p50_us, 4);
        assert_eq!(s.p99_us, 41);
        assert_eq!(s.max_us, 1_500);
        assert!(s.within_target());

        // Two slow calls in a hundred push p99 over the target.
        times.record(1_200_000);
        let s = times.summary();
        assert_eq!(s.p99_us, 1_201);
        assert!(!s.within_target());
    }

    #[test]
    fn very_slow_calls_land_in_the_last_bucket() {
        let times = HookTimes::new();
        times.record(250_000_000);
        let s = times.summary();
        assert_eq!(s.max_us, 250_000);
        assert_eq!(s.p99_us, BUCKET_LIMIT_US as u64 + 1);
    }

    #[test]
    fn a_summary_is_due_every_window() {
        let times = HookTimes::new();
        let due: Vec<u64> = (1..=WINDOW + 5).filter(|_| times.record(2_000)).collect();
        assert_eq!(due.len(), 1);
        assert_eq!(times.count(), WINDOW + 5);
        let s = times.take();
        assert_eq!(s.count, WINDOW + 5);
        assert_eq!(times.count(), 0);
        assert_eq!(times.summary(), HookSummary::default());
        // The next window is due again after WINDOW calls.
        assert_eq!((0..WINDOW).filter(|_| times.record(2_000)).count(), 1);
    }

    #[test]
    fn summary_text_holds_only_numbers() {
        let times = HookTimes::new();
        for ns in [800, 2_000, 15_000, 900_000] {
            times.record(ns);
        }
        let text = times.summary().to_string();
        assert_eq!(text, "4 calls, p50 3 us, p99 900 us, max 900 us");
        let words: Vec<&str> = text
            .split(|c: char| c.is_ascii_digit() || c == ',' || c == ' ')
            .filter(|w| !w.is_empty())
            .collect();
        assert_eq!(words, ["calls", "p", "us", "p", "us", "max", "us"]);
    }
}
