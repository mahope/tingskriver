//! Danish display formats. Callers supply local dates/times; no timezone conversion.
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

const MONTHS: [&str; 12] = [
    "januar",
    "februar",
    "marts",
    "april",
    "maj",
    "juni",
    "juli",
    "august",
    "september",
    "oktober",
    "november",
    "december",
];
const SHORT_MONTHS: [&str; 12] = [
    "jan.", "feb.", "mar.", "apr.", "maj", "jun.", "jul.", "aug.", "sep.", "okt.", "nov.", "dec.",
];
const DAYS: [&str; 7] = [
    "mandag", "tirsdag", "onsdag", "torsdag", "fredag", "lørdag", "søndag",
];

pub fn date(value: NaiveDate) -> String {
    format!(
        "{}. {} {}",
        value.day(),
        MONTHS[value.month0() as usize],
        value.year()
    )
}

pub fn date_with_weekday(value: NaiveDate) -> String {
    format!(
        "{} den {}",
        DAYS[value.weekday().num_days_from_monday() as usize],
        date(value)
    )
}

pub fn short_date(value: NaiveDate) -> String {
    format!(
        "{}. {} {}",
        value.day(),
        SHORT_MONTHS[value.month0() as usize],
        value.year()
    )
}

pub fn time(value: NaiveTime) -> String {
    format!("kl. {:02}.{:02}", value.hour(), value.minute())
}

pub fn date_time(value: NaiveDateTime) -> String {
    format!("{} {}", date(value.date()), time(value.time()))
}

fn grouped(value: &str) -> String {
    let mut result = String::new();
    let digits = value.trim_start_matches('-');
    if value.starts_with('-') {
        result.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            result.push('.');
        }
        result.push(c);
    }
    result
}

/// Exact integer formatting, including values outside f64's integer precision.
pub fn integer(value: i64) -> String {
    grouped(&value.to_string())
}

/// Fixed precision, capped at 15 decimals. Non-finite input displays an em dash.
pub fn number(value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let raw = format!("{:.*}", decimals.min(15), value);
    let (whole, fraction) = raw.split_once('.').unwrap_or((&raw, ""));
    let whole = if whole == "-0" && fraction.chars().all(|c| c == '0') {
        "0"
    } else {
        whole
    };
    let mut result = grouped(whole);
    if !fraction.is_empty() {
        result.push(',');
        result.push_str(fraction);
    }
    result
}

/// The input is a percentage (12.5 means 12,5 %), not a ratio.
pub fn percent(value: f64, decimals: usize) -> String {
    format!("{} %", number(value, decimals))
}

/// Whole elapsed seconds; seconds are omitted once a full minute has elapsed.
pub fn duration(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3599 => format!("{} min", seconds / 60),
        _ if seconds % 3600 < 60 => format!("{} t", seconds / 3600),
        _ => format!("{} t {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// Decimal SI units: 1 KB = 1,000 bytes. One decimal above bytes.
pub fn file_size(bytes: u64) -> String {
    let (divisor, unit) = if bytes >= 1_000_000_000 {
        (1_000_000_000, "GB")
    } else if bytes >= 1_000_000 {
        (1_000_000, "MB")
    } else if bytes >= 1_000 {
        (1_000, "KB")
    } else {
        return format!("{bytes} B");
    };
    format!("{} {unit}", number(bytes as f64 / divisor as f64, 1))
}

#[cfg(test)]
mod tests;
