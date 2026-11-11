//! Channel isolation between the microphone track and the system track (#189).
//!
//! A pilot tone is played into one track only and the other track is checked for
//! it. A muted or mis-routed channel shows up as leakage (or as a missing pilot)
//! even when the level meter looks healthy. The analysis is pure: it takes two
//! sample buffers, the phase times and the thresholds, so it runs on synthetic
//! signals, on the output of the synthetic capture device (`recording_soak
//! isolation`) and on a real recording (`kanalisolation` example).
//!
//! The thresholds live in `docs/kanalisolation-graenser.json`, which the tools
//! and the release checklist (`docs/release-checkliste.md`) both refer to.
//!
//! The method (a pilot per channel, three phases, isolation in dB, clipped share
//! and duration difference) is inspired by the open source anarlog Linux audio
//! test (MIT, Fastrepl, Inc.). No code is copied.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Amplitudes below this are treated as silence (-120 dBFS), so a perfectly
/// silent channel gives a finite, very large isolation.
const AMPLITUDE_FLOOR: f64 = 1e-6;
/// A sample at or beyond this fraction of full scale counts as clipped.
const CLIP_LEVEL: f32 = 0.999;
/// The least analysed audio per phase, after the guard time has been removed.
const MIN_ANALYSIS_SECONDS: f64 = 0.5;

/// The thresholds file, `docs/kanalisolation-graenser.json`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Thresholds {
    /// Pilot tone on the microphone track.
    pub microphone_hz: f64,
    /// Pilot tone on the system track.
    pub system_hz: f64,
    /// Seconds ignored at both ends of every phase (switching, filters settling).
    pub phase_guard_s: f64,
    /// Least isolation in the microphone track while both pilots play.
    #[serde(rename = "mic_isolation_db_min")]
    pub microphone_isolation_db_min: f64,
    /// Least isolation in the system track while both pilots play.
    #[serde(rename = "speaker_isolation_db_min")]
    pub system_isolation_db_min: f64,
    /// Least isolation of the system track while only the microphone pilot plays.
    #[serde(rename = "mic_pilot_isolation_db_min")]
    pub microphone_pilot_isolation_db_min: f64,
    /// Least isolation of the microphone track while only the system pilot plays.
    pub system_tone_isolation_db_min: f64,
    /// Cross-track isolation of either pilot in a single-pilot phase.
    pub cross_channel_isolation_db_min: f64,
    /// The own pilot must be at least this loud in its own track (dBFS), or the
    /// channel counts as muted and an isolation figure proves nothing.
    pub min_pilot_dbfs: f64,
    /// Largest share of clipped samples in either track.
    pub max_clipped_share: f64,
    /// Largest difference in duration between the two tracks.
    pub max_duration_diff_s: f64,
}

impl Thresholds {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let thresholds: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid thresholds: {e}"))?;
        thresholds.validate()?;
        Ok(thresholds)
    }

    fn validate(&self) -> Result<(), String> {
        let positive = [
            ("microphone_hz", self.microphone_hz),
            ("system_hz", self.system_hz),
            ("max_duration_diff_s", self.max_duration_diff_s),
        ];
        for (name, value) in positive {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be a positive number"));
            }
        }
        if (self.microphone_hz - self.system_hz).abs() < 100.0 {
            return Err("the two pilot tones must be at least 100 Hz apart".into());
        }
        let finite = [
            ("phase_guard_s", self.phase_guard_s),
            (
                "microphone_isolation_db_min",
                self.microphone_isolation_db_min,
            ),
            ("system_isolation_db_min", self.system_isolation_db_min),
            (
                "microphone_pilot_isolation_db_min",
                self.microphone_pilot_isolation_db_min,
            ),
            (
                "system_tone_isolation_db_min",
                self.system_tone_isolation_db_min,
            ),
            (
                "cross_channel_isolation_db_min",
                self.cross_channel_isolation_db_min,
            ),
            ("min_pilot_dbfs", self.min_pilot_dbfs),
            ("max_clipped_share", self.max_clipped_share),
        ];
        for (name, value) in finite {
            if !value.is_finite() {
                return Err(format!("{name} must be a number"));
            }
        }
        if self.phase_guard_s < 0.0 || !(0.0..=1.0).contains(&self.max_clipped_share) {
            return Err("phase_guard_s must be >= 0 and max_clipped_share within 0..1".into());
        }
        let db_floors = [
            (
                "cross_channel_isolation_db_min",
                self.cross_channel_isolation_db_min,
            ),
            (
                "microphone_isolation_db_min",
                self.microphone_isolation_db_min,
            ),
            ("system_isolation_db_min", self.system_isolation_db_min),
            (
                "microphone_pilot_isolation_db_min",
                self.microphone_pilot_isolation_db_min,
            ),
            (
                "system_tone_isolation_db_min",
                self.system_tone_isolation_db_min,
            ),
        ];
        for (name, value) in db_floors {
            if value < 0.0 {
                return Err(format!("{name} must not be negative"));
            }
        }
        if self.min_pilot_dbfs > 0.0 {
            return Err("min_pilot_dbfs must be <= 0 (dBFS)".into());
        }
        Ok(())
    }
}

/// Which pilots play in a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    Both,
    MicrophoneOnly,
    SystemOnly,
}

/// A phase on the shared recording clock, in seconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Phase {
    pub kind: PhaseKind,
    pub start_s: f64,
    pub end_s: f64,
}

impl Phase {
    /// `both:0:3`, `microphone:3:6` or `system:6:9` (kind, start seconds, end seconds).
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<_> = text.split(':').collect();
        let [kind, start, end] = parts.as_slice() else {
            return Err(format!("phase '{text}' must look like both:0:3"));
        };
        let kind = match *kind {
            "both" => PhaseKind::Both,
            "mic" | "microphone" => PhaseKind::MicrophoneOnly,
            "system" => PhaseKind::SystemOnly,
            other => {
                return Err(format!(
                    "unknown phase '{other}' (both, microphone or system)"
                ))
            }
        };
        let number = |s: &str| {
            s.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(|| format!("invalid time '{s}' in phase '{text}'"))
        };
        let (start_s, end_s) = (number(start)?, number(end)?);
        if end_s <= start_s {
            return Err(format!("phase '{text}' ends before it starts"));
        }
        Ok(Self {
            kind,
            start_s,
            end_s,
        })
    }
}

/// One isolation figure compared with its threshold.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Measurement {
    pub label: String,
    pub isolation_db: f64,
    pub min_db: f64,
    pub pilot_dbfs: f64,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseReport {
    pub phase: Phase,
    pub measurements: Vec<Measurement>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub phases: Vec<PhaseReport>,
    pub clipped_share_microphone: f64,
    pub clipped_share_system: f64,
    pub duration_diff_s: f64,
    pub failures: Vec<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// Plain text for the console and the release attachment.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for report in &self.phases {
            let _ = writeln!(
                out,
                "phase {:?} {:.2}-{:.2} s",
                report.phase.kind, report.phase.start_s, report.phase.end_s
            );
            for m in &report.measurements {
                let _ = writeln!(
                    out,
                    "  {:<28} {:>7.1} dB (min {:.1}, pilot {:.1} dBFS) {}",
                    m.label,
                    m.isolation_db,
                    m.min_db,
                    m.pilot_dbfs,
                    if m.ok { "ok" } else { "FAIL" }
                );
            }
        }
        let _ = writeln!(
            out,
            "clipped share: microphone {:.5}, system {:.5}; duration difference {:.3} s",
            self.clipped_share_microphone, self.clipped_share_system, self.duration_diff_s
        );
        if self.passed() {
            out.push_str("PASS\n");
        } else {
            for failure in &self.failures {
                let _ = writeln!(out, "FAIL: {failure}");
            }
        }
        out
    }
}

/// Amplitude (linear, 0..1 for a full scale sine) of one frequency in `samples`,
/// found with the Goertzel algorithm under a Hann window.
pub fn tone_amplitude(samples: &[f32], rate: u32, hz: f64) -> f64 {
    let n = samples.len();
    if n < 2 {
        return 0.0;
    }
    let omega = 2.0 * std::f64::consts::PI * hz / f64::from(rate);
    let coeff = 2.0 * omega.cos();
    let (mut s1, mut s2, mut window_sum) = (0.0f64, 0.0f64, 0.0f64);
    let denom = (n - 1) as f64;
    for (i, sample) in samples.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / denom).cos();
        window_sum += w;
        let s = f64::from(*sample) * w + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
    2.0 * power.sqrt() / window_sum
}

fn db(amplitude: f64) -> f64 {
    20.0 * amplitude.max(AMPLITUDE_FLOOR).log10()
}

fn clipped_share(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().filter(|s| s.abs() >= CLIP_LEVEL).count() as f64 / samples.len() as f64
}

fn slice<'a>(samples: &'a [f32], rate: u32, phase: &Phase, guard: f64) -> Option<&'a [f32]> {
    if rate == 0
        || !phase.start_s.is_finite()
        || !phase.end_s.is_finite()
        || phase.start_s < 0.0
        || phase.end_s <= phase.start_s
        || phase.end_s > samples.len() as f64 / f64::from(rate)
    {
        return None;
    }
    let from = ((phase.start_s + guard) * f64::from(rate)).round() as usize;
    let to = ((phase.end_s - guard) * f64::from(rate)).round() as usize;
    (to > from && (to - from) as f64 / f64::from(rate) >= MIN_ANALYSIS_SECONDS)
        .then(|| &samples[from..to])
}

/// Find the three ordered pilot phases in 100 ms Goertzel windows. Leading and
/// trailing silence are allowed; missing, reordered or interrupted phases fail.
/// Boundaries have at most 100 ms resolution; runs shorter than 300 ms are
/// switching transients. The phase guard excludes the remaining switch window.
pub fn detect_phases(
    microphone: &[f32],
    system: &[f32],
    rate: u32,
    thresholds: &Thresholds,
) -> Result<Vec<Phase>, String> {
    if rate == 0 {
        return Err("sample rate must be positive".into());
    }
    let window = (rate as usize / 10).max(2);
    let energies: Vec<_> = microphone
        .chunks_exact(window)
        .zip(system.chunks_exact(window))
        .map(|(m, s)| {
            (
                tone_amplitude(m, rate, thresholds.microphone_hz),
                tone_amplitude(s, rate, thresholds.system_hz),
            )
        })
        .collect();
    let peak_m = energies.iter().map(|e| e.0).fold(0.0, f64::max);
    let peak_s = energies.iter().map(|e| e.1).fold(0.0, f64::max);
    if db(peak_m) < thresholds.min_pilot_dbfs || db(peak_s) < thresholds.min_pilot_dbfs {
        return Err("cannot detect phases: microphone or system pilot missing".into());
    }
    let floor = 10f64.powf(thresholds.min_pilot_dbfs / 20.0);
    let mut runs: Vec<(Option<PhaseKind>, usize, usize)> = Vec::new();
    for (i, (m, s)) in energies.into_iter().enumerate() {
        let kind = match (
            m >= (peak_m * 0.2).max(floor),
            s >= (peak_s * 0.2).max(floor),
        ) {
            (true, true) => Some(PhaseKind::Both),
            (true, false) => Some(PhaseKind::MicrophoneOnly),
            (false, true) => Some(PhaseKind::SystemOnly),
            _ => None,
        };
        if let Some(last) = runs.last_mut().filter(|last| last.0 == kind) {
            last.2 = i + 1;
        } else {
            runs.push((kind, i, i + 1));
        }
    }
    let mut phases = Vec::new();
    let mut ended = false;
    // Mixed windows around a switch are transient, not an extra pilot phase.
    for (kind, from, to) in runs.into_iter().filter(|run| run.2 - run.1 >= 3) {
        let Some(kind) = kind else {
            ended |= !phases.is_empty();
            continue;
        };
        if ended {
            return Err("cannot detect phases: silence interrupts pilot sequence".into());
        }
        phases.push(Phase {
            kind,
            start_s: (from * window) as f64 / f64::from(rate),
            end_s: (to * window) as f64 / f64::from(rate),
        });
    }
    let kinds: Vec<_> = phases.iter().map(|p| p.kind).collect();
    if kinds
        != [
            PhaseKind::Both,
            PhaseKind::MicrophoneOnly,
            PhaseKind::SystemOnly,
        ]
    {
        return Err("cannot detect phases: expected both, microphone, system in order; supply explicit phases to diagnose routing".into());
    }
    Ok(phases)
}

/// Measure isolation per phase, clipped share and the duration difference.
/// `microphone` and `system` are mono samples on the same time axis and sample rate.
/// Each full phase must fit both recordings, including its guard time.
pub fn analyze(
    microphone: &[f32],
    system: &[f32],
    rate: u32,
    phases: &[Phase],
    thresholds: &Thresholds,
) -> Report {
    let t = thresholds;
    let mut failures = Vec::new();
    let mut reports = Vec::new();
    if phases.is_empty() {
        failures.push("no phases given".to_string());
    }
    for phase in phases {
        let (Some(m), Some(s)) = (
            slice(microphone, rate, phase, t.phase_guard_s),
            slice(system, rate, phase, t.phase_guard_s),
        ) else {
            failures.push(format!(
                "{:?} {:.2}-{:.2} s: too short or outside the recording (need {MIN_ANALYSIS_SECONDS} s after the guard)",
                phase.kind, phase.start_s, phase.end_s
            ));
            continue;
        };
        let at = |track: &[f32], hz| tone_amplitude(track, rate, hz);
        let (mic_own, mic_other) = (at(m, t.microphone_hz), at(m, t.system_hz));
        let (sys_own, sys_other) = (at(s, t.system_hz), at(s, t.microphone_hz));
        // (label, isolation, threshold, level of the pilot that must be present)
        let mut rows: Vec<(&str, f64, f64, f64)> = match phase.kind {
            PhaseKind::Both => vec![
                (
                    "system pilot in microphone track",
                    db(mic_own) - db(mic_other),
                    t.microphone_isolation_db_min,
                    db(mic_own),
                ),
                (
                    "microphone pilot in system track",
                    db(sys_own) - db(sys_other),
                    t.system_isolation_db_min,
                    db(sys_own),
                ),
            ],
            PhaseKind::MicrophoneOnly => vec![(
                "microphone pilot in system track",
                db(mic_own) - db(sys_other),
                t.microphone_pilot_isolation_db_min,
                db(mic_own),
            )],
            PhaseKind::SystemOnly => vec![(
                "system pilot in microphone track",
                db(sys_own) - db(mic_other),
                t.system_tone_isolation_db_min,
                db(sys_own),
            )],
        };
        if let Some(&(_, isolation, _, pilot)) =
            rows.first().filter(|_| phase.kind != PhaseKind::Both)
        {
            let cross_label = if phase.kind == PhaseKind::MicrophoneOnly {
                "cross-channel microphone pilot"
            } else {
                "cross-channel system pilot"
            };
            rows.push((
                cross_label,
                isolation,
                t.cross_channel_isolation_db_min,
                pilot,
            ));
        }
        let measurements: Vec<_> = rows
            .into_iter()
            .map(|(label, isolation_db, min_db, pilot_dbfs)| {
                let present = pilot_dbfs >= t.min_pilot_dbfs;
                let ok = present && isolation_db >= min_db;
                if !present {
                    failures.push(format!(
                        "{:?}: {label}: own pilot missing ({pilot_dbfs:.1} dBFS < {:.1}); channel muted or mis-routed",
                        phase.kind, t.min_pilot_dbfs
                    ));
                } else if !ok {
                    failures.push(format!(
                        "{:?}: {label}: isolation {isolation_db:.1} dB < {min_db:.1} dB",
                        phase.kind
                    ));
                }
                Measurement {
                    label: label.to_string(),
                    isolation_db,
                    min_db,
                    pilot_dbfs,
                    ok,
                }
            })
            .collect();
        reports.push(PhaseReport {
            phase: phase.clone(),
            measurements,
        });
    }
    let (clipped_mic, clipped_system) = (clipped_share(microphone), clipped_share(system));
    let duration_diff_s = (microphone.len() as f64 - system.len() as f64).abs() / f64::from(rate);
    for (name, share) in [("microphone", clipped_mic), ("system", clipped_system)] {
        if share > t.max_clipped_share {
            failures.push(format!(
                "{name} track clipped share {share:.5} > {}",
                t.max_clipped_share
            ));
        }
    }
    if duration_diff_s > t.max_duration_diff_s {
        failures.push(format!(
            "track durations differ by {duration_diff_s:.3} s > {} s",
            t.max_duration_diff_s
        ));
    }
    Report {
        phases: reports,
        clipped_share_microphone: clipped_mic,
        clipped_share_system: clipped_system,
        duration_diff_s,
        failures,
    }
}

/// A sine of `amplitude` (0..1), for synthetic fixtures and the synthetic device.
pub fn sine(rate: u32, hz: f64, amplitude: f32, start_sample: u64, count: usize) -> Vec<f32> {
    (0..count)
        .map(|i| {
            let t = (start_sample + i as u64) as f64 / f64::from(rate);
            amplitude * (2.0 * std::f64::consts::PI * hz * t).sin() as f32
        })
        .collect()
}

/// Read a mono (first channel) 16-bit PCM or 32-bit float WAV file as `f32`.
/// Returns the samples and the sample rate. Small on purpose: the tool only
/// needs the tracks that the recorder itself writes.
pub fn read_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
    let u16_at = |o: usize| {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |o: usize| {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("not a RIFF/WAVE file".into());
    }
    let (mut format, mut offset) = (None, 12usize);
    while let (Some(id), Some(size)) = (bytes.get(offset..offset + 4), u32_at(offset + 4)) {
        let body = offset + 8;
        let size = size as usize;
        if id == b"fmt " {
            format = Some((
                u16_at(body).ok_or("short fmt chunk")?,
                u16_at(body + 2).ok_or("short fmt chunk")?,
                u32_at(body + 4).ok_or("short fmt chunk")?,
                u16_at(body + 14).ok_or("short fmt chunk")?,
            ));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or("data before fmt chunk")?;
            let channels = usize::from(channels.max(1));
            // A writer that was killed leaves a too large size: use what is there.
            let end = bytes.len().min(body.saturating_add(size));
            let data = bytes.get(body..end).ok_or("short data chunk")?;
            let samples: Vec<f32> = match (tag, bits) {
                (1, 16) => data
                    .chunks_exact(2 * channels)
                    .map(|f| f32::from(i16::from_le_bytes([f[0], f[1]])) / 32768.0)
                    .collect(),
                (3, 32) => data
                    .chunks_exact(4 * channels)
                    .map(|f| f32::from_le_bytes([f[0], f[1], f[2], f[3]]))
                    .collect(),
                _ => return Err(format!("unsupported WAV format {tag}/{bits} bit")),
            };
            return Ok((samples, rate));
        }
        offset = body.saturating_add(size).saturating_add(size & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const THRESHOLDS: &str = include_str!("../../../../docs/kanalisolation-graenser.json");
    const CHECKLIST: &str = include_str!("../../../../docs/release-checkliste.md");

    fn thresholds() -> Thresholds {
        Thresholds::from_json(THRESHOLDS).expect("thresholds file")
    }

    fn phases() -> Vec<Phase> {
        ["both:0:3", "microphone:3:6", "system:6:9"]
            .iter()
            .map(|p| Phase::parse(p).expect("phase"))
            .collect()
    }

    /// Nine seconds: both pilots, only the microphone pilot, only the system pilot.
    /// `leak` is the linear share of the other channel's pilot that bleeds in.
    fn fixture(leak_into_mic: f32, leak_into_system: f32) -> (Vec<f32>, Vec<f32>) {
        let t = thresholds();
        let n = RATE as usize * 9;
        let mic_tone = sine(RATE, t.microphone_hz, 0.25, 0, n);
        let sys_tone = sine(RATE, t.system_hz, 0.25, 0, n);
        let second = RATE as usize;
        let (mut microphone, mut sys) = (vec![0.0; n], vec![0.0; n]);
        for i in 0..n {
            let (mic_on, sys_on) = match i / (3 * second) {
                0 => (true, true),
                1 => (true, false),
                _ => (false, true),
            };
            if mic_on {
                microphone[i] += mic_tone[i];
                sys[i] += mic_tone[i] * leak_into_system;
            }
            if sys_on {
                sys[i] += sys_tone[i];
                microphone[i] += sys_tone[i] * leak_into_mic;
            }
        }
        (microphone, sys)
    }

    #[test]
    fn isolation_detects_phases_after_one_second_delay() {
        for delay in [1.0, 1.05] {
            isolation_detects_delayed_phases(delay);
        }
    }

    fn isolation_detects_delayed_phases(delay: f64) {
        let (m, s) = fixture(0.0, 0.0);
        let mut microphone = vec![0.0; (RATE as f64 * delay) as usize];
        let mut system = microphone.clone();
        microphone.extend(m);
        system.extend(s);
        microphone.extend(vec![0.0; RATE as usize]);
        system.extend(vec![0.0; RATE as usize]);
        let phases = detect_phases(&microphone, &system, RATE, &thresholds()).unwrap();
        assert_eq!(phases.len(), 3);
        for (phase, start) in phases.iter().zip([delay, delay + 3.0, delay + 6.0]) {
            assert!((phase.start_s - start).abs() < 0.11, "{phase:?}");
            assert!((phase.end_s - start - 3.0).abs() < 0.11, "{phase:?}");
        }
        let report = analyze(&microphone, &system, RATE, &phases, &thresholds());
        assert!(report.passed(), "{}", report.to_text());
        assert!(detect_phases(&[], &[], RATE, &thresholds()).is_err());
    }

    #[test]
    fn isolation_uses_all_five_spec_thresholds() {
        let (microphone, system) = fixture(0.1, 0.1);
        let report = analyze(&microphone, &system, RATE, &phases(), &thresholds());
        let limits: Vec<_> = report
            .phases
            .iter()
            .flat_map(|p| &p.measurements)
            .map(|m| m.min_db)
            .collect();
        assert_eq!(limits, [12.0, 15.0, 6.0, 12.0, 18.0, 12.0]);
    }

    #[test]
    fn isolation_enforces_each_spec_threshold() {
        let (microphone, system) = fixture(0.1, 0.1);
        for index in 0..5 {
            let mut t = thresholds();
            let limits = [
                &mut t.microphone_isolation_db_min,
                &mut t.system_isolation_db_min,
                &mut t.system_tone_isolation_db_min,
                &mut t.microphone_pilot_isolation_db_min,
                &mut t.cross_channel_isolation_db_min,
            ];
            *limits.into_iter().nth(index).unwrap() = 21.0;
            let report = analyze(&microphone, &system, RATE, &phases(), &t);
            assert!(!report.passed(), "threshold {index} was ignored");
        }
    }

    #[test]
    fn isolation_microphone_filter_level_matches_harness_contract() {
        use crate::audio::audio_processing::{HighPassFilter, LoudnessNormalizer};
        let mut filter = HighPassFilter::new(RATE, 80.0);
        let mut normalizer = LoudnessNormalizer::new(1, RATE).unwrap();
        let mut output = Vec::new();
        for block in 0..400 {
            let input = sine(RATE, 523.0, 0.25, block * 480, 480);
            output.extend(normalizer.normalize_loudness(&filter.process(&input)));
        }
        for (start, end) in [(0.25, 1.75), (2.25, 3.75)] {
            let samples = &output[(start * RATE as f64) as usize..(end * RATE as f64) as usize];
            let level = db(tone_amplitude(samples, RATE, 523.0));
            assert!(
                (level + 19.0).abs() <= 2.0,
                "microphone pilot {level:.1} dBFS outside harness contract -19 +/- 2 dBFS"
            );
        }
    }

    #[test]
    fn clean_fixture_passes_with_large_isolation() {
        let (microphone, sys) = fixture(0.0, 0.0);
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        assert!(report.passed(), "{}", report.to_text());
        assert_eq!(report.phases.len(), 3);
        for m in report.phases.iter().flat_map(|p| &p.measurements) {
            assert!(m.isolation_db > 60.0, "{m:?}");
            assert!((m.pilot_dbfs + 12.0).abs() < 0.5, "{m:?}");
        }
    }

    #[test]
    fn leakage_of_the_system_pilot_into_the_mic_fails() {
        // 0.3 of the pilot is about 10.5 dB below it: below the routing limits.
        let (microphone, sys) = fixture(0.3, 0.0);
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        assert!(!report.passed());
        assert!(
            report
                .failures
                .iter()
                .any(|f| f.contains("system pilot in microphone track")),
            "{:?}",
            report.failures
        );
        // The clean direction is not blamed.
        assert!(!report
            .failures
            .iter()
            .any(|f| f.contains("microphone pilot in system track")));
    }

    #[test]
    fn leakage_of_the_mic_pilot_into_the_system_fails() {
        let (microphone, sys) = fixture(0.0, 0.3);
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        assert!(!report.passed());
        assert!(report
            .failures
            .iter()
            .any(|f| f.contains("microphone pilot in system track")));
    }

    #[test]
    fn isolation_is_measured_in_db() {
        // 0.1 amplitude ratio is 20 dB: passes 12/15/18 dB, fails nothing.
        let (microphone, sys) = fixture(0.1, 0.1);
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        let both = &report.phases[0].measurements;
        for m in both {
            assert!((m.isolation_db - 20.0).abs() < 0.5, "{m:?}");
        }
        // 20 dB also passes the stricter 18 dB system-only limit.
        assert!(report.passed(), "{}", report.to_text());
    }

    #[test]
    fn muted_channel_is_not_reported_as_isolated() {
        // Silent microphone track: nothing leaks, but the pilot is missing too.
        let (_, sys) = fixture(0.0, 0.0);
        let microphone = vec![0.0; sys.len()];
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        assert!(!report.passed());
        assert!(report
            .failures
            .iter()
            .any(|f| f.contains("own pilot missing")));
    }

    #[test]
    fn clipping_and_duration_difference_fail() {
        let (mut microphone, mut sys) = fixture(0.0, 0.0);
        for sample in microphone.iter_mut().take(1000) {
            *sample = 1.0;
        }
        sys.truncate(sys.len() - RATE as usize / 4);
        let report = analyze(&microphone, &sys, RATE, &phases(), &thresholds());
        assert!(report.failures.iter().any(|f| f.contains("clipped share")));
        assert!(report
            .failures
            .iter()
            .any(|f| f.contains("durations differ")));
        assert!((report.duration_diff_s - 0.25).abs() < 1e-9);
    }

    #[test]
    fn phase_must_fit_both_recordings_before_guard() {
        let (microphone, system) = fixture(0.0, 0.0);
        let t = thresholds();
        assert!(analyze(&microphone, &system, RATE, &phases(), &t).passed());
        for end_s in [30.0, 9.0 + t.phase_guard_s / 2.0] {
            let mut phases = phases();
            phases[2].end_s = end_s;
            let report = analyze(&microphone, &system, RATE, &phases, &t);
            assert!(!report.passed(), "end {end_s}: {report:?}");
            assert!(report
                .failures
                .iter()
                .any(|f| f.contains("outside the recording")));
        }
        for (mic, sys) in [
            (&microphone[..microphone.len() - 1], &system[..]),
            (&microphone[..], &system[..system.len() - 1]),
        ] {
            let report = analyze(mic, sys, RATE, &phases(), &t);
            assert!(!report.passed(), "{report:?}");
            assert!(report
                .failures
                .iter()
                .any(|f| f.contains("outside the recording")));
        }
    }

    #[test]
    fn short_phase_and_missing_phases_fail() {
        let (microphone, sys) = fixture(0.0, 0.0);
        let short = [Phase::parse("both:0:0.6").expect("phase")];
        assert!(!analyze(&microphone, &sys, RATE, &short, &thresholds()).passed());
        assert!(!analyze(&microphone, &sys, RATE, &[], &thresholds()).passed());
    }

    #[test]
    fn phase_parsing_rejects_bad_input() {
        assert!(Phase::parse("both:0:3").is_ok());
        assert_eq!(
            Phase::parse("mic:3:6").unwrap(),
            Phase::parse("microphone:3:6").unwrap()
        );
        for bad in ["both:3:3", "x:0:1", "both:0", "both:a:1", "microphone:-1:2"] {
            assert!(Phase::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn thresholds_reject_invalid_files() {
        assert!(Thresholds::from_json("{}").is_err());
        let mut t = thresholds();
        t.system_hz = t.microphone_hz + 10.0;
        let json = serde_json::to_string(&t).expect("json");
        assert!(Thresholds::from_json(&json).is_err());

        let mut negative = thresholds();
        negative.microphone_pilot_isolation_db_min = -3.0;
        let json = serde_json::to_string(&negative).expect("json");
        assert!(Thresholds::from_json(&json).is_err());

        let mut loud = thresholds();
        loud.min_pilot_dbfs = 6.0;
        let json = serde_json::to_string(&loud).expect("json");
        assert!(Thresholds::from_json(&json).is_err());
    }

    #[test]
    fn release_checklist_uses_the_same_thresholds_file() {
        assert!(CHECKLIST.contains("kanalisolation-graenser.json"));
    }

    #[test]
    fn wav_reader_reads_the_recorders_tracks() {
        let samples: Vec<i16> = vec![0, 16384, -16384, 32767];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + 8u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] {
            wav.extend_from_slice(&v.to_le_bytes());
        }
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        // Too large size, as a killed writer leaves it.
        wav.extend_from_slice(&1_000_000u32.to_le_bytes());
        for s in &samples {
            wav.extend_from_slice(&s.to_le_bytes());
        }
        let (read, rate) = read_wav(&wav).expect("wav");
        assert_eq!(rate, RATE);
        assert_eq!(read.len(), 4);
        assert!((read[1] - 0.5).abs() < 1e-4);
        assert!(read_wav(b"nope").is_err());
    }
}
