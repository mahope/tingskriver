use super::timezone::Zone;
use super::{recurrence, Attendee, CalendarConfig, CalendarError, Event};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, CalendarError>;
pub(super) fn invalid<T>() -> Result<T> {
    Err(CalendarError::Parse)
}

#[derive(Clone)]
pub(super) struct Property {
    pub name: String,
    pub params: BTreeMap<String, String>,
    pub value: String,
    pub resolved_zone: Option<Zone>,
}
#[derive(Clone)]
pub(super) struct Stamp {
    pub local: NaiveDateTime,
    pub zone: Zone,
    pub date: bool,
}
impl Stamp {
    pub fn utc(&self) -> Result<DateTime<Utc>> {
        self.at(self.local)
    }
    pub fn at(&self, local: NaiveDateTime) -> Result<DateTime<Utc>> {
        // RFC 5545: choose the first occurrence in an autumn fold. Nonexistent
        // recurrence times are skipped by the recurrence expander.
        self.zone.at(local)
    }
}
pub(super) fn zone(name: &str) -> Result<Zone> {
    let mapped = match name {
        "W. Europe Standard Time"
        | "Romance Standard Time"
        | "Central European Standard Time"
        | "Central Europe Standard Time" => "Europe/Copenhagen",
        "GMT Standard Time" => "Europe/London",
        "Eastern Standard Time" => "America/New_York",
        "Pacific Standard Time" => "America/Los_Angeles",
        "UTC" => "UTC",
        other => other,
    };
    mapped
        .parse()
        .map(Zone::Iana)
        .map_err(|_| CalendarError::Parse)
}
fn split_quoted(s: &str, delimiter: char) -> Result<Vec<&str>> {
    let mut quoted = false;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c == delimiter && !quoted {
            parts.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    if quoted {
        return invalid();
    }
    parts.push(&s[start..]);
    Ok(parts)
}
fn property(line: &str) -> Result<Property> {
    let mut quoted = false;
    let pos = line
        .char_indices()
        .find_map(|(i, c)| {
            if c == '"' {
                quoted = !quoted;
            }
            (c == ':' && !quoted).then_some(i)
        })
        .ok_or(CalendarError::Parse)?;
    let parts = split_quoted(&line[..pos], ';')?;
    let mut params = BTreeMap::new();
    for part in &parts[1..] {
        let (k, v) = part.split_once('=').ok_or(CalendarError::Parse)?;
        params.insert(k.to_ascii_uppercase(), v.trim_matches('"').to_owned());
    }
    Ok(Property {
        name: parts[0].to_ascii_uppercase(),
        params,
        value: line[pos + 1..].into(),
        resolved_zone: None,
    })
}
fn text(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => result.push('\n'),
                Some(c) => result.push(c),
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}
pub(super) fn stamp(p: &Property, default_zone: Zone) -> Result<Stamp> {
    let date = p.params.get("VALUE").is_some_and(|v| v == "DATE");
    let utc = p.value.ends_with('Z');
    let zone = if utc {
        Zone::Iana(chrono_tz::UTC)
    } else if let Some(zone) = &p.resolved_zone {
        zone.clone()
    } else if let Some(name) = p.params.get("TZID") {
        zone(name)?
    } else {
        default_zone
    };
    let local = if date {
        NaiveDate::parse_from_str(&p.value, "%Y%m%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    } else {
        NaiveDateTime::parse_from_str(p.value.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()
    }
    .ok_or(CalendarError::Parse)?;
    Ok(Stamp { local, zone, date })
}
fn duration(s: &str) -> Result<(Duration, Duration)> {
    let Some(rest) = s.strip_prefix('P') else {
        return invalid();
    };
    let mut time = false;
    let mut n = String::new();
    let mut seconds = 0_i64;
    let mut days = 0_i64;
    let mut any = false;
    for c in rest.chars() {
        if c == 'T' && n.is_empty() && !time {
            time = true;
            continue;
        }
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        let unit = match (time, c) {
            (false, 'W') => 7,
            (false, 'D') => 1,
            (true, 'H') => 3600,
            (true, 'M') => 60,
            (true, 'S') => 1,
            _ => return invalid(),
        };
        let value = n.parse::<i64>().map_err(|_| CalendarError::Parse)?;
        let target = if time { &mut seconds } else { &mut days };
        *target = target
            .checked_add(value.checked_mul(unit).ok_or(CalendarError::Parse)?)
            .ok_or(CalendarError::Parse)?;
        n.clear();
        any = true;
    }
    if !any || !n.is_empty() {
        return invalid();
    }
    Ok((
        Duration::try_days(days).ok_or(CalendarError::Parse)?,
        Duration::try_seconds(seconds).ok_or(CalendarError::Parse)?,
    ))
}
#[derive(Clone)]
pub(super) struct RawEvent {
    pub props: Vec<Property>,
}
impl RawEvent {
    pub fn get(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name == name)
    }
    fn uid(&self) -> Result<&str> {
        self.get("UID")
            .map(|p| p.value.as_str())
            .filter(|s| !s.is_empty())
            .ok_or(CalendarError::Parse)
    }
    fn excluded(&self, config: &CalendarConfig) -> bool {
        self.get("STATUS")
            .is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED"))
            || self.props.iter().filter(|p| p.name == "ATTENDEE").any(|p| {
                p.params
                    .get("PARTSTAT")
                    .is_some_and(|s| s.eq_ignore_ascii_case("DECLINED"))
                    && config
                        .own_addresses
                        .iter()
                        .any(|own| email(&p.value).eq_ignore_ascii_case(own.trim()))
            })
    }
    fn event(&self, start: &Stamp, begin: DateTime<Utc>) -> Result<Event> {
        let local = start.zone.local(begin)?;
        let end = if let Some(end) = self.get("DTEND") {
            if self.get("DURATION").is_some() {
                return invalid();
            }
            let end = stamp(end, start.zone.clone())?;
            if end.date != start.date {
                return invalid();
            }
            if start.date {
                start.at(local
                    .checked_add_signed(end.local - start.local)
                    .ok_or(CalendarError::Parse)?)?
            } else {
                begin
                    .checked_add_signed(end.utc()? - start.utc()?)
                    .ok_or(CalendarError::Parse)?
            }
        } else {
            let (days, seconds) = if let Some(d) = self.get("DURATION") {
                duration(&d.value)?
            } else if start.date {
                (Duration::days(1), Duration::zero())
            } else {
                (Duration::zero(), Duration::zero())
            };
            // Day durations are nominal wall-clock days, including DST changes.
            (if days == Duration::zero() {
                begin
            } else {
                start.at(local.checked_add_signed(days).ok_or(CalendarError::Parse)?)?
            })
            .checked_add_signed(seconds)
            .ok_or(CalendarError::Parse)?
        };
        if end < begin {
            return invalid();
        }
        Ok(Event {
            uid: self.uid()?.into(),
            title: self
                .get("SUMMARY")
                .map(|p| text(&p.value))
                .unwrap_or_default(),
            attendees: self
                .props
                .iter()
                .filter(|p| p.name == "ATTENDEE")
                .map(|p| Attendee {
                    name: p.params.get("CN").map(|s| text(s)),
                    email: email(&p.value).into(),
                    declined: p
                        .params
                        .get("PARTSTAT")
                        .is_some_and(|v| v.eq_ignore_ascii_case("DECLINED")),
                })
                .collect(),
            start: begin,
            end,
            all_day: start.date,
        })
    }
}
fn email(s: &str) -> &str {
    if s.get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("mailto:"))
    {
        &s[7..]
    } else {
        s
    }
}

/// Expand occurrences overlapping [from, to). Unknown recurrence rules and
/// timezone IDs fail closed; the caller can keep its previous valid cache.
pub fn parse(
    source: &str,
    config: &CalendarConfig,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<Event>> {
    if source.len() > 8 * 1024 * 1024 || to <= from {
        return invalid();
    }
    let default_zone = zone(&config.timezone)?;
    let mut lines: Vec<String> = Vec::new();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
            } else {
                return invalid();
            }
        } else {
            lines.push(line.trim_end_matches('\r').to_owned());
        }
    }
    let mut stack = Vec::new();
    let mut current = None;
    let mut raws = Vec::new();
    let mut timezone_id: Option<String> = None;
    let mut observance = None;
    let mut observances = Vec::new();
    let mut zones = BTreeMap::new();
    let mut calendar = false;
    for line in lines.iter().filter(|s| !s.is_empty()) {
        let p = property(line)?;
        if p.name == "BEGIN" {
            if p.value == "VCALENDAR" {
                if calendar || !stack.is_empty() {
                    return invalid();
                }
                calendar = true;
            }
            if p.value == "VEVENT" {
                if current.is_some() || stack.last().map(String::as_str) != Some("VCALENDAR") {
                    return invalid();
                }
                current = Some(RawEvent { props: Vec::new() });
            }
            if matches!(p.value.as_str(), "STANDARD" | "DAYLIGHT")
                && stack.last().map(String::as_str) == Some("VTIMEZONE")
            {
                observance = Some(RawEvent { props: Vec::new() });
            }
            stack.push(p.value);
            continue;
        }
        if p.name == "END" {
            if stack.pop().as_deref() != Some(&p.value) {
                return invalid();
            }
            if p.value == "VEVENT" {
                raws.push(current.take().ok_or(CalendarError::Parse)?);
            }
            if matches!(p.value.as_str(), "STANDARD" | "DAYLIGHT") {
                observances.push(observance.take().ok_or(CalendarError::Parse)?);
            }
            if p.value == "VTIMEZONE" {
                let id = timezone_id.take().ok_or(CalendarError::Parse)?;
                let resolved = match zone(&id) {
                    Ok(known) => known,
                    Err(_) => Zone::custom(&observances, to + Duration::days(366))?,
                };
                if zones.insert(id, resolved).is_some() {
                    return invalid();
                }
                observances.clear();
            }
            continue;
        }
        if stack.last().is_some_and(|s| s == "VEVENT") {
            if let Some(raw) = current.as_mut() {
                raw.props.push(p);
            }
        } else if stack
            .last()
            .is_some_and(|s| matches!(s.as_str(), "STANDARD" | "DAYLIGHT"))
        {
            if let Some(raw) = observance.as_mut() {
                raw.props.push(p);
            }
        } else if stack.last().is_some_and(|s| s == "VTIMEZONE") && p.name == "TZID" {
            timezone_id = Some(p.value);
        }
    }
    if !calendar || !stack.is_empty() || current.is_some() {
        return invalid();
    }
    for raw in &mut raws {
        for p in &mut raw.props {
            if let Some(id) = p.params.get("TZID") {
                p.resolved_zone = zones.get(id).cloned();
            }
        }
    }
    let mut masters = BTreeSet::new();
    for raw in &raws {
        if raw.get("RECURRENCE-ID").is_none() && !masters.insert(raw.uid()?) {
            return invalid();
        }
    }
    let mut overrides = BTreeMap::new();
    for raw in &raws {
        if let Some(id) = raw.get("RECURRENCE-ID") {
            if id.params.contains_key("RANGE") {
                return invalid();
            }
            let master_zone = raws
                .iter()
                .find(|r| r.uid().ok() == raw.uid().ok() && r.get("RECURRENCE-ID").is_none())
                .and_then(|r| r.get("DTSTART"))
                .map(|p| stamp(p, default_zone.clone()))
                .transpose()?
                .map(|s| s.zone)
                .unwrap_or_else(|| default_zone.clone());
            let key = (raw.uid()?.to_owned(), stamp(id, master_zone)?.utc()?);
            if overrides.insert(key, raw).is_some() {
                return invalid();
            }
        }
    }
    let mut result = Vec::new();
    for raw in &raws {
        if raw.get("RECURRENCE-ID").is_some() || raw.excluded(config) {
            continue;
        }
        let start = stamp(
            raw.get("DTSTART").ok_or(CalendarError::Parse)?,
            default_zone.clone(),
        )?;
        start.utc()?;
        let mut dates: BTreeSet<_> = recurrence::expand(raw, &start, to)?
            .into_iter()
            .filter_map(|local| start.at(local).ok())
            .collect();
        let mut excluded = BTreeSet::new();
        for p in raw
            .props
            .iter()
            .filter(|p| p.name == "EXDATE" || p.name == "RDATE")
        {
            for value in p.value.split(',') {
                let mut p = p.clone();
                p.value = value.into();
                let d = stamp(&p, start.zone.clone())?;
                if p.name == "EXDATE" {
                    excluded.insert(d.utc()?);
                } else {
                    dates.insert(d.utc()?);
                }
            }
        }
        for utc in dates {
            if excluded.contains(&utc) || overrides.contains_key(&(raw.uid()?.to_owned(), utc)) {
                continue;
            }
            let event = raw.event(&start, utc)?;
            if event.start < to
                && (event.end > from || event.end == event.start && event.start >= from)
            {
                result.push(event);
            }
        }
    }
    for raw in overrides.values() {
        if raw.excluded(config) {
            continue;
        }
        let start = stamp(
            raw.get("DTSTART").ok_or(CalendarError::Parse)?,
            default_zone.clone(),
        )?;
        let event = raw.event(&start, start.utc()?)?;
        if event.start < to && (event.end > from || event.end == event.start && event.start >= from)
        {
            result.push(event);
        }
    }
    result.sort_by(|a, b| a.start.cmp(&b.start).then(a.uid.cmp(&b.uid)));
    Ok(result)
}
