use super::*;

#[test]
fn danish_dates_and_times() {
    let november = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
    assert_eq!(date(november), "3. november 2026");
    assert_eq!(date_with_weekday(november), "tirsdag den 3. november 2026");
    assert_eq!(short_date(november), "3. nov. 2026");
    assert_eq!(
        date(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
        "1. december 2026"
    );
    assert_eq!(
        date(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
        "31. januar 2026"
    );
    assert_eq!(time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()), "kl. 00.00");
    assert_eq!(
        time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
        "kl. 12.00"
    );
    assert_eq!(
        date_time(november.and_hms_opt(14, 30, 0).unwrap()),
        "3. november 2026 kl. 14.30"
    );
    for (month, name) in [
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
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            date(NaiveDate::from_ymd_opt(2026, month as u32 + 1, 1).unwrap()),
            format!("1. {name} 2026")
        );
    }
}

#[test]
fn danish_numbers() {
    for (value, decimals, expected) in [
        (1000.5, 2, "1.000,50"),
        (999.0, 0, "999"),
        (1_234_567.89, 2, "1.234.567,89"),
        (-1000.5, 2, "-1.000,50"),
        (999.999, 2, "1.000,00"),
        (0.125, 2, "0,12"),
        (1.875, 2, "1,88"),
        (-0.001, 2, "0,00"),
        (0.0, 0, "0"),
    ] {
        assert_eq!(number(value, decimals), expected);
    }
    assert_eq!(integer(i64::MIN), "-9.223.372.036.854.775.808");
    assert_eq!(integer(1_000_000), "1.000.000");
    assert_eq!(percent(12.5, 1), "12,5 %");
    assert_eq!(number(f64::NAN, 2), "—");
    assert_eq!(number(f64::INFINITY, 2), "—");
    assert_eq!(number(1.0, usize::MAX), "1,000000000000000");
}

#[test]
fn danish_durations_and_sizes() {
    for (seconds, expected) in [
        (0, "0 s"),
        (12, "12 s"),
        (59, "59 s"),
        (60, "1 min"),
        (2700, "45 min"),
        (3599, "59 min"),
        (3600, "1 t"),
        (3659, "1 t"),
        (3660, "1 t 1 min"),
        (3900, "1 t 5 min"),
    ] {
        assert_eq!(duration(seconds), expected);
    }
    for (bytes, expected) in [
        (0, "0 B"),
        (999, "999 B"),
        (1000, "1,0 KB"),
        (1500, "1,5 KB"),
        (1_250_000, "1,2 MB"),
        (1_000_000_000, "1,0 GB"),
    ] {
        assert_eq!(file_size(bytes), expected);
    }
}
