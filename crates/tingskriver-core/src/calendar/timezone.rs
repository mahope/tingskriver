use super::{
    ics::{stamp, RawEvent},
    recurrence, CalendarError,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub(super) enum Zone {
    Iana(Tz),
    Custom(Arc<CustomZone>),
}
pub(super) struct CustomZone {
    transitions: Vec<(DateTime<Utc>, i32)>,
    initial: i32,
    offsets: BTreeSet<i32>,
}
impl Zone {
    pub fn local(&self, utc: DateTime<Utc>) -> Result<NaiveDateTime, CalendarError> {
        match self {
            Self::Iana(zone) => Ok(utc.with_timezone(zone).naive_local()),
            Self::Custom(zone) => utc
                .naive_utc()
                .checked_add_signed(Duration::seconds(i64::from(zone.offset_at(utc))))
                .ok_or(CalendarError::Parse),
        }
    }
    pub fn at(&self, local: NaiveDateTime) -> Result<DateTime<Utc>, CalendarError> {
        match self {
            Self::Iana(zone) => zone
                .from_local_datetime(&local)
                .earliest()
                .map(|d| d.with_timezone(&Utc))
                .ok_or(CalendarError::Parse),
            Self::Custom(zone) => zone
                .offsets
                .iter()
                .filter_map(|offset| {
                    let utc = local
                        .checked_sub_signed(Duration::seconds(i64::from(*offset)))?
                        .and_utc();
                    (zone.offset_at(utc) == *offset).then_some(utc)
                })
                .min()
                .ok_or(CalendarError::Parse),
        }
    }
    pub fn custom(observances: &[RawEvent], to: DateTime<Utc>) -> Result<Self, CalendarError> {
        let mut transitions = Vec::new();
        let mut initial = None;
        let mut offsets = BTreeSet::new();
        for raw in observances {
            let from = offset(&raw.get("TZOFFSETFROM").ok_or(CalendarError::Parse)?.value)?;
            let next = offset(&raw.get("TZOFFSETTO").ok_or(CalendarError::Parse)?.value)?;
            offsets.insert(from);
            offsets.insert(next);
            let start = stamp(
                raw.get("DTSTART").ok_or(CalendarError::Parse)?,
                Self::Iana(chrono_tz::UTC),
            )?;
            let dates = recurrence::expand(raw, &start, to)?;
            let mut dates = dates;
            for p in raw.props.iter().filter(|p| p.name == "RDATE") {
                for value in p.value.split(',') {
                    let mut p = p.clone();
                    p.value = value.into();
                    dates.insert(stamp(&p, Self::Iana(chrono_tz::UTC))?.local);
                }
            }
            for local in dates {
                let utc = local
                    .checked_sub_signed(Duration::seconds(i64::from(from)))
                    .ok_or(CalendarError::Parse)?
                    .and_utc();
                if initial.is_none_or(|(first, _)| utc < first) {
                    initial = Some((utc, from));
                }
                transitions.push((utc, next));
            }
        }
        transitions.sort_unstable();
        let initial = initial.ok_or(CalendarError::Parse)?.1;
        Ok(Self::Custom(Arc::new(CustomZone {
            transitions,
            initial,
            offsets,
        })))
    }
}
impl CustomZone {
    fn offset_at(&self, utc: DateTime<Utc>) -> i32 {
        let i = self.transitions.partition_point(|(at, _)| *at <= utc);
        if i == 0 {
            self.initial
        } else {
            self.transitions[i - 1].1
        }
    }
}
fn offset(s: &str) -> Result<i32, CalendarError> {
    if !s.is_ascii() || !matches!(s.len(), 5 | 7) || !matches!(s.as_bytes()[0], b'+' | b'-') {
        return Err(CalendarError::Parse);
    }
    let hours = s[1..3].parse::<i32>().map_err(|_| CalendarError::Parse)?;
    let minutes = s[3..5].parse::<i32>().map_err(|_| CalendarError::Parse)?;
    let seconds = if s.len() == 7 {
        s[5..7].parse::<i32>().map_err(|_| CalendarError::Parse)?
    } else {
        0
    };
    if hours > 23 || minutes > 59 || seconds > 59 {
        return Err(CalendarError::Parse);
    }
    let n = (hours * 3600 + minutes * 60 + seconds) * if s.starts_with('-') { -1 } else { 1 };
    FixedOffset::east_opt(n).ok_or(CalendarError::Parse)?;
    Ok(n)
}
