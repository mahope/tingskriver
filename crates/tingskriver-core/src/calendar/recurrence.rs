use super::{
    ics::{invalid, stamp, Property, RawEvent, Stamp},
    CalendarError,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, CalendarError>;

pub(super) fn expand(
    raw: &RawEvent,
    start: &Stamp,
    to: DateTime<Utc>,
) -> Result<BTreeSet<NaiveDateTime>> {
    let mut dates = BTreeSet::from([start.local]);
    let rules: Vec<_> = raw.props.iter().filter(|p| p.name == "RRULE").collect();
    if rules.len() > 1 || raw.get("EXRULE").is_some() {
        return invalid();
    }
    let Some(rule) = rules.first() else {
        return Ok(dates);
    };
    let mut fields = BTreeMap::new();
    for part in rule.value.split(';') {
        let (k, v) = part.split_once('=').ok_or(CalendarError::Parse)?;
        if !matches!(
            k,
            "FREQ" | "INTERVAL" | "COUNT" | "UNTIL" | "BYDAY" | "BYMONTHDAY" | "BYMONTH" | "WKST"
        ) || fields.insert(k, v).is_some()
        {
            return invalid();
        }
    }
    let freq = *fields.get("FREQ").ok_or(CalendarError::Parse)?;
    if !matches!(freq, "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY") {
        return invalid();
    }
    let number = |key: &str, default: u32| -> Result<u32> {
        let n = fields
            .get(key)
            .map(|v| v.parse::<u32>())
            .transpose()
            .map_err(|_| CalendarError::Parse)?
            .unwrap_or(default);
        if n == 0 {
            invalid()
        } else {
            Ok(n)
        }
    };
    let interval = number("INTERVAL", 1)?;
    let count = number("COUNT", u32::MAX)?;
    if fields.contains_key("COUNT") && fields.contains_key("UNTIL") {
        return invalid();
    }
    let until = fields
        .get("UNTIL")
        .map(|s| {
            let mut p = Property {
                name: "UNTIL".into(),
                params: BTreeMap::new(),
                value: (*s).into(),
                resolved_zone: None,
            };
            if s.len() == 8 {
                p.params.insert("VALUE".into(), "DATE".into());
            }
            stamp(&p, start.zone.clone())
        })
        .transpose()?;
    let wkst = weekday(fields.get("WKST").copied().unwrap_or("MO"))?;
    let months = integers(fields.get("BYMONTH").copied(), 1, 12, false)?;
    let monthdays = integers(fields.get("BYMONTHDAY").copied(), -31, 31, true)?;
    if freq == "WEEKLY" && !monthdays.is_empty() {
        return invalid();
    }
    let mut days = Vec::new();
    if let Some(value) = fields.get("BYDAY") {
        for item in value.split(',') {
            if item.len() < 2 || !item.is_ascii() {
                return invalid();
            }
            let (ord, day) = item.split_at(item.len() - 2);
            let ordinal = if ord.is_empty() {
                0
            } else {
                ord.parse::<i32>().map_err(|_| CalendarError::Parse)?
            };
            if !ord.is_empty()
                && (ordinal == 0
                    || ordinal.unsigned_abs() > 53
                    || matches!(freq, "DAILY" | "WEEKLY"))
            {
                return invalid();
            }
            days.push((ordinal, weekday(day)?));
        }
    }
    // Bounded work even for hostile feeds. 200,000 days covers > 500 years.
    let stop = start.zone.local(to)?.date();
    let mut date = start.local.date();
    let mut seen = 1_u32;
    let mut steps = 0;
    while date <= stop && seen < count {
        steps += 1;
        if steps > 200_000 {
            return invalid();
        }
        let local = date.and_time(start.local.time());
        if local > start.local {
            let delta = (date - start.local.date()).num_days();
            let md = (date.year() - start.local.year()) * 12 + date.month() as i32
                - start.local.month() as i32;
            let sd = start.local.weekday().num_days_from_monday() as i64;
            let shift = (sd + 7 - i64::from(wkst)) % 7;
            let period = match freq {
                "DAILY" => delta % i64::from(interval) == 0,
                "WEEKLY" => (delta + shift) / 7 % i64::from(interval) == 0,
                "MONTHLY" => i64::from(md) % i64::from(interval) == 0,
                "YEARLY" => i64::from(date.year() - start.local.year()) % i64::from(interval) == 0,
                _ => false,
            };
            let month_ok = if months.is_empty() {
                freq != "YEARLY"
                    || !days.is_empty()
                    || !monthdays.is_empty()
                    || date.month() == start.local.month()
            } else {
                months.contains(&(date.month() as i32))
            };
            let last = last_day(date)?;
            let md_ok = monthdays.is_empty()
                || monthdays.iter().any(|n| {
                    if *n > 0 {
                        date.day() as i32 == *n
                    } else {
                        date.day() as i32 == last as i32 + 1 + *n
                    }
                });
            let day_ok = if days.is_empty() {
                match freq {
                    "WEEKLY" => date.weekday() == start.local.weekday(),
                    "MONTHLY" | "YEARLY" if monthdays.is_empty() => date.day() == start.local.day(),
                    _ => true,
                }
            } else {
                days.iter().any(|(ord, wd)| {
                    if date.weekday().num_days_from_monday() != *wd {
                        return false;
                    }
                    if *ord == 0 {
                        return true;
                    }
                    let (day, total) = if freq == "YEARLY" && months.is_empty() {
                        (
                            date.ordinal(),
                            NaiveDate::from_ymd_opt(date.year(), 12, 31)
                                .map(|d| d.ordinal())
                                .unwrap_or(365),
                        )
                    } else {
                        (date.day(), last)
                    };
                    if *ord > 0 {
                        (day as i32 - 1) / 7 + 1 == *ord
                    } else {
                        -((total as i32 - day as i32) / 7 + 1) == *ord
                    }
                })
            };
            if period && month_ok && md_ok && day_ok {
                if let Some(until) = &until {
                    let beyond = if until.date {
                        date > until.local.date()
                    } else {
                        match start.at(local) {
                            Ok(d) => d > until.utc()?,
                            Err(_) => false,
                        }
                    };
                    if beyond {
                        break;
                    }
                }
                if start.at(local).is_ok() {
                    dates.insert(local);
                    seen += 1;
                }
            }
        }
        date = date
            .checked_add_signed(Duration::days(1))
            .ok_or(CalendarError::Parse)?;
    }
    Ok(dates)
}
fn weekday(s: &str) -> Result<u32> {
    match s {
        "MO" => Ok(0),
        "TU" => Ok(1),
        "WE" => Ok(2),
        "TH" => Ok(3),
        "FR" => Ok(4),
        "SA" => Ok(5),
        "SU" => Ok(6),
        _ => invalid(),
    }
}
fn integers(s: Option<&str>, min: i32, max: i32, no_zero: bool) -> Result<Vec<i32>> {
    s.map(|s| {
        s.split(',')
            .map(|s| {
                let n = s.parse::<i32>().map_err(|_| CalendarError::Parse)?;
                if n < min || n > max || no_zero && n == 0 {
                    invalid()
                } else {
                    Ok(n)
                }
            })
            .collect()
    })
    .unwrap_or_else(|| Ok(Vec::new()))
}
fn last_day(d: NaiveDate) -> Result<u32> {
    (28..=31)
        .rev()
        .find(|n| NaiveDate::from_ymd_opt(d.year(), d.month(), *n).is_some())
        .ok_or(CalendarError::Parse)
}
