//! Dictation timings per step, and their median and 95th percentile per
//! provider. Only milliseconds, never text.

use std::collections::BTreeSet;

/// Target from release of the shortcut to text in the program.
pub const TARGET_MS: u64 = 1500;
/// Number of most recent dictations per provider in the statistics.
pub const WINDOW: usize = 50;
/// A warning needs at least this many dictations, so a single slow first
/// dictation (model load) does not trigger it.
pub const MIN_SAMPLES: usize = 5;

/// Milliseconds spent in each step of one dictation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Steps {
    /// Shortcut pressed until the microphone was open.
    pub start_ms: Option<u64>,
    /// Length of the recording.
    pub audio_ms: u64,
    /// Shortcut released until the audio was ready for transcription.
    pub capture_ms: u64,
    /// Transcription call, including a fallback.
    pub transcribe_ms: u64,
    /// LLM polishing, when it ran.
    pub polish_ms: Option<u64>,
    /// Insertion until the text reached the program.
    pub insert_ms: u64,
    /// Shortcut released until the text reached the program.
    pub total_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// "syv" or "local": the engine that produced the text.
    pub provider: String,
    pub steps: Steps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub median_ms: u64,
    pub p95_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderStats {
    pub provider: String,
    pub count: usize,
    pub total: Stat,
    pub start: Option<Stat>,
    pub capture: Stat,
    pub transcribe: Stat,
    pub polish: Option<Stat>,
    pub insert: Stat,
    /// Median total above [`TARGET_MS`] with at least [`MIN_SAMPLES`] dictations.
    pub over_target: bool,
}

/// Median; the mean of the two middle values for an even count (as
/// `scripts/maal-ydeevne.ps1`).
pub fn median(values: &[u64]) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2),
    }
}

/// Nearest-rank percentile, `p` in 0..=100 (as `scripts/maal-ydeevne.ps1`).
pub fn percentile(values: &[u64], p: u64) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    if sorted.is_empty() {
        return None;
    }
    let rank = (p.min(100) as usize * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

fn stat(values: &[u64]) -> Option<Stat> {
    Some(Stat {
        median_ms: median(values)?,
        p95_ms: percentile(values, 95)?,
    })
}

/// Statistics per provider over its [`WINDOW`] newest samples. `samples`
/// must be ordered newest first. Providers are sorted by name.
pub fn summarize(samples: &[Sample]) -> Vec<ProviderStats> {
    let providers: BTreeSet<&str> = samples.iter().map(|s| s.provider.as_str()).collect();
    providers
        .into_iter()
        .filter_map(|provider| {
            let steps: Vec<&Steps> = samples
                .iter()
                .filter(|s| s.provider == provider)
                .take(WINDOW)
                .map(|s| &s.steps)
                .collect();
            let collect = |f: fn(&Steps) -> u64| steps.iter().map(|s| f(s)).collect::<Vec<_>>();
            let optional = |f: fn(&Steps) -> Option<u64>| steps.iter().filter_map(|s| f(s)).collect::<Vec<_>>();
            let total = stat(&collect(|s| s.total_ms))?;
            Some(ProviderStats {
                provider: provider.to_string(),
                count: steps.len(),
                over_target: steps.len() >= MIN_SAMPLES && total.median_ms > TARGET_MS,
                total,
                start: stat(&optional(|s| s.start_ms)),
                capture: stat(&collect(|s| s.capture_ms))?,
                transcribe: stat(&collect(|s| s.transcribe_ms))?,
                polish: stat(&optional(|s| s.polish_ms)),
                insert: stat(&collect(|s| s.insert_ms))?,
            })
        })
        .collect()
}

/// Providers that crossed the target since the last check. `warned` holds the
/// providers already warned about and is updated: a provider is warned again
/// only after its median has been back under the target.
pub fn new_warnings<'a>(stats: &'a [ProviderStats], warned: &mut BTreeSet<String>) -> Vec<&'a ProviderStats> {
    let mut fresh = Vec::new();
    for s in stats {
        if s.over_target {
            if warned.insert(s.provider.clone()) {
                fresh.push(s);
            }
        } else {
            warned.remove(&s.provider);
        }
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(provider: &str, total_ms: u64) -> Sample {
        Sample {
            provider: provider.to_string(),
            steps: Steps {
                start_ms: Some(40),
                audio_ms: 15_000,
                capture_ms: 20,
                transcribe_ms: total_ms.saturating_sub(120),
                polish_ms: None,
                insert_ms: 100,
                total_ms,
            },
        }
    }

    #[test]
    fn median_and_percentile_match_the_script() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[5]), Some(5));
        assert_eq!(median(&[4, 1, 3, 2]), Some(2)); // (2 + 3) / 2, rounded down
        assert_eq!(median(&[10, 30, 20]), Some(20));
        let hundred: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&hundred, 95), Some(95));
        assert_eq!(percentile(&[7], 95), Some(7));
        assert_eq!(percentile(&[1, 2, 3], 0), Some(1));
        assert_eq!(percentile(&[], 95), None);
    }

    #[test]
    fn steps_aggregate_per_provider() {
        let mut samples: Vec<Sample> = (0..10).map(|i| sample("syv", 800 + i * 10)).collect();
        samples.extend((0..4).map(|i| sample("local", 1200 + i * 100)));
        samples[0].steps.polish_ms = Some(600);
        let stats = summarize(&samples);
        assert_eq!(stats.len(), 2);

        let local = &stats[0];
        assert_eq!(local.provider, "local");
        assert_eq!(local.count, 4);
        assert_eq!(local.total, Stat { median_ms: 1350, p95_ms: 1500 });
        assert_eq!(local.polish, None);
        assert!(!local.over_target);

        let syv = &stats[1];
        assert_eq!(syv.count, 10);
        assert_eq!(syv.total, Stat { median_ms: 845, p95_ms: 890 });
        assert_eq!(syv.transcribe, Stat { median_ms: 725, p95_ms: 770 });
        assert_eq!(syv.capture, Stat { median_ms: 20, p95_ms: 20 });
        assert_eq!(syv.insert, Stat { median_ms: 100, p95_ms: 100 });
        assert_eq!(syv.start, Some(Stat { median_ms: 40, p95_ms: 40 }));
        assert_eq!(syv.polish, Some(Stat { median_ms: 600, p95_ms: 600 }));
    }

    #[test]
    fn only_the_newest_window_counts() {
        // Newest first: 50 fast ones, then 30 old slow ones.
        let mut samples: Vec<Sample> = (0..WINDOW).map(|_| sample("syv", 900)).collect();
        samples.extend((0..30).map(|_| sample("syv", 5000)));
        let stats = summarize(&samples);
        assert_eq!(stats[0].count, WINDOW);
        assert_eq!(stats[0].total.p95_ms, 900);
    }

    #[test]
    fn median_over_target_warns_once_until_it_recovers() {
        let mut warned = BTreeSet::new();
        let slow: Vec<Sample> = (0..MIN_SAMPLES).map(|_| sample("local", 1600)).collect();
        let stats = summarize(&slow);
        assert!(stats[0].over_target);
        let fresh = new_warnings(&stats, &mut warned);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].total.median_ms, 1600);
        // Still slow: no new warning.
        assert!(new_warnings(&stats, &mut warned).is_empty());

        // Back under the target, then slow again: warns again.
        let fast = summarize(&[sample("local", 900)]);
        assert!(new_warnings(&fast, &mut warned).is_empty());
        assert_eq!(new_warnings(&stats, &mut warned).len(), 1);
    }

    #[test]
    fn exactly_the_target_or_too_few_samples_do_not_warn() {
        let at_target: Vec<Sample> = (0..MIN_SAMPLES).map(|_| sample("syv", TARGET_MS)).collect();
        assert!(!summarize(&at_target)[0].over_target);
        let few: Vec<Sample> = (0..MIN_SAMPLES - 1).map(|_| sample("syv", 4000)).collect();
        assert!(!summarize(&few)[0].over_target);
    }
}
