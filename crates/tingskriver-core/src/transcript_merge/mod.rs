//! Pure, deterministic merging of microphone and system transcript segments.

use std::{borrow::Cow, collections::HashMap};

pub const DEFAULT_MICROPHONE_LABEL: &str = "Mig";
pub const DEFAULT_UNKNOWN_LABEL: &str = "Ukendt taler";
pub const NUMBERED_SPEAKER_PREFIX: &str = "Taler";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub slut_ms: u64,
    pub tekst: String,
    pub taler: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MergeConfig {
    pub mikrofon_label: String,
    pub system_diariseret: bool,
    pub ukendt_label: String,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            mikrofon_label: DEFAULT_MICROPHONE_LABEL.into(),
            system_diariseret: false,
            ukendt_label: DEFAULT_UNKNOWN_LABEL.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taler {
    Mig,
    Nummereret(usize),
    Ukendt,
}

impl Taler {
    /// Resolve a display label without storing a label string per segment.
    pub fn label<'a>(&self, config: &'a MergeConfig) -> Cow<'a, str> {
        match self {
            Self::Mig => Cow::Borrowed(&config.mikrofon_label),
            Self::Nummereret(n) => Cow::Owned(format!("{NUMBERED_SPEAKER_PREFIX} {n}")),
            Self::Ukendt => Cow::Borrowed(&config.ukendt_label),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum Kilde {
    Mikrofon,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedSegment {
    pub start_ms: u64,
    pub slut_ms: u64,
    pub tekst: String,
    pub taler: Taler,
    pub kilde: Kilde,
    pub overlapper: bool,
    pub taler_mangler: bool,
}

/// Sort and merge without changing inputs. Equal starts retain input order,
/// with microphone segments before system segments. Intervals are half-open.
pub fn merge(mic: &[Segment], system: &[Segment], config: &MergeConfig) -> Vec<MergedSegment> {
    let mut ordered: Vec<_> = mic
        .iter()
        .map(|s| (s, Kilde::Mikrofon))
        .chain(system.iter().map(|s| (s, Kilde::System)))
        .collect();
    ordered.sort_by_key(|(s, _)| s.start_ms);
    let mut speakers = HashMap::new();
    let mut output = Vec::with_capacity(ordered.len());
    for (s, kilde) in ordered {
        let taler = if kilde == Kilde::Mikrofon {
            Taler::Mig
        } else if let Some(id) = s
            .taler
            .as_deref()
            .filter(|id| config.system_diariseret && !id.trim().is_empty())
        {
            let next = speakers.len() + 1;
            Taler::Nummereret(*speakers.entry(id).or_insert(next))
        } else {
            Taler::Ukendt
        };
        output.push(MergedSegment {
            start_ms: s.start_ms,
            slut_ms: s.slut_ms.max(s.start_ms),
            tekst: s.tekst.clone(),
            taler,
            kilde,
            overlapper: false,
            taler_mangler: taler == Taler::Ukendt,
        });
    }
    // A previous maximum end catches nesting; the next nonempty interval
    // catches future overlaps, even with zero-length segments in between.
    let mut max_end = 0;
    for s in &mut output {
        if s.start_ms < s.slut_ms {
            s.overlapper = s.start_ms < max_end;
            max_end = max_end.max(s.slut_ms);
        }
    }
    let mut next_start = None;
    for s in output.iter_mut().rev() {
        if s.start_ms < s.slut_ms {
            s.overlapper |= next_start.is_some_and(|start| start < s.slut_ms);
            next_start = Some(s.start_ms);
        }
    }
    output
}

#[cfg(test)]
mod tests;
