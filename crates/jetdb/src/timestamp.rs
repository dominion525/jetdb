/// Timestamp (f64, days since 1899-12-30) → date/time conversion utilities.
///
/// Microsoft Access stores date/time values as IEEE 754 double-precision
/// floating-point numbers representing fractional days since the epoch
/// 1899-12-30 00:00:00.  The integer part is the day count and the
/// fractional part is the time of day.  Before the epoch the value is
/// negative, and the time of day is the absolute value of the fractional
/// part: -1.25 is 1899-12-29 06:00:00, a day before the epoch at 6 a.m., as
/// Access shows it (not 1899-12-28 18:00:00, 1.25 days before the epoch).
/// Access epoch expressed as Julian Day Number (1899-12-30).
const ACCESS_EPOCH_JDN: i64 = 2_415_019;

/// Seconds in a day.
const SECS_PER_DAY: f64 = 86_400.0;

/// Convert an Access timestamp to calendar parts.
///
/// Returns `(year, month, day, hour, minute, second)`.
pub fn timestamp_to_parts(ts: f64) -> (i32, u32, u32, u32, u32, u32) {
    if !ts.is_finite() {
        return (1899, 12, 30, 0, 0, 0); // epoch
    }
    // The day is the integer part toward zero and the time of day the
    // absolute value of the fraction, also for a negative value (see the
    // module comment).
    let mut days = ts.trunc() as i64;
    let frac = ts.fract().abs();
    let mut total_secs = (frac * SECS_PER_DAY + 0.5) as u32; // round
    if total_secs >= SECS_PER_DAY as u32 {
        // Rounded up to midnight: the time of day runs forward from the
        // date, so this is the next date for a negative value too.
        days += 1;
        total_secs = 0;
    }

    let jdn = ACCESS_EPOCH_JDN + days;
    let (year, month, day) = jdn_to_gregorian(jdn);

    let hour = total_secs / 3600;
    let minute = (total_secs % 3600) / 60;
    let second = total_secs % 60;

    (year, month, day, hour, minute, second)
}

/// The format of a timestamp whose time is midnight, as `jetdb export` writes
/// it by default.
pub const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// The format of a timestamp with a time of day, as `jetdb export` writes it
/// by default.
pub const DEFAULT_DATETIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Format an Access timestamp as `jetdb export` writes it by default: the
/// date alone ([`DEFAULT_DATE_FORMAT`]) when the time is midnight, and the
/// date and time ([`DEFAULT_DATETIME_FORMAT`]) otherwise.
pub fn format_default(ts: f64) -> String {
    let format = if is_date_only(ts) {
        DEFAULT_DATE_FORMAT
    } else {
        DEFAULT_DATETIME_FORMAT
    };
    format_timestamp(ts, format)
}

/// Format an Access timestamp using a strftime-like format string.
///
/// Supported directives: `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%%`.
pub fn format_timestamp(ts: f64, fmt: &str) -> String {
    let (year, month, day, hour, minute, second) = timestamp_to_parts(ts);
    let mut result = String::with_capacity(fmt.len() + 8);
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{year:04}")),
                Some('m') => result.push_str(&format!("{month:02}")),
                Some('d') => result.push_str(&format!("{day:02}")),
                Some('H') => result.push_str(&format!("{hour:02}")),
                Some('M') => result.push_str(&format!("{minute:02}")),
                Some('S') => result.push_str(&format!("{second:02}")),
                Some('%') => result.push('%'),
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Return `true` if the timestamp has no time component (date only).
///
/// Tolerates floating-point noise up to 1e-9.
pub fn is_date_only(ts: f64) -> bool {
    if !ts.is_finite() {
        return true;
    }
    let frac = (ts - ts.floor()).abs();
    frac < 1e-9
}

// ---------------------------------------------------------------------------
// DateTimeExtended support
// ---------------------------------------------------------------------------

/// DateTimeExtended epoch (0001-01-01) expressed as Julian Day Number.
const EXT_DATETIME_EPOCH_JDN: i64 = 1_721_426;

/// Format a DateTimeExtended value as an ISO 8601 string.
///
/// `days` is the number of days since epoch 0001-01-01,
/// `seconds` is the elapsed seconds within that day,
/// `nanos100` is the sub-second fraction in 100-nanosecond units.
pub fn format_ext_datetime(days: i64, seconds: i64, nanos100: i64) -> String {
    let (y, m, d) = jdn_to_gregorian(EXT_DATETIME_EPOCH_JDN + days);
    let h = seconds / 3600;
    let min = (seconds % 3600) / 60;
    let sec = seconds % 60;
    if seconds == 0 && nanos100 == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else if nanos100 == 0 {
        format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{sec:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{sec:02}.{nanos100:07}")
    }
}

/// Convert Julian Day Number to Gregorian (year, month, day).
///
/// Uses the algorithm from Wikipedia "Julian day" § Converting Gregorian
/// calendar date from Julian day number.
fn jdn_to_gregorian(jdn: i64) -> (i32, u32, u32) {
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146_097;
    let c = a - (146_097 * b) / 4;

    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;

    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;

    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero() {
        let (y, m, d, h, min, s) = timestamp_to_parts(0.0);
        assert_eq!((y, m, d, h, min, s), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn known_date_2003_01_02() {
        // 2003-01-02 is day 37623 from 1899-12-30
        let ts = 37623.0;
        let (y, m, d, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2003, 1, 2));
        assert_eq!((h, min, s), (0, 0, 0));
    }

    #[test]
    fn date_with_time() {
        // 0.5 = noon
        let ts = 37623.5;
        let (y, m, d, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2003, 1, 2));
        assert_eq!((h, min, s), (12, 0, 0));
    }

    #[test]
    fn date_with_time_detailed() {
        // 0.75 = 18:00
        let ts = 37623.75;
        let (_, _, _, h, min, s) = timestamp_to_parts(ts);
        assert_eq!((h, min, s), (18, 0, 0));
    }

    #[test]
    fn leap_year_feb29() {
        // 2000-02-29 is day 36585 from 1899-12-30
        let ts = 36585.0;
        let (y, m, d, _, _, _) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (2000, 2, 29));
    }

    #[test]
    fn negative_value() {
        // Day -1 = 1899-12-29
        let ts = -1.0;
        let (y, m, d, _, _, _) = timestamp_to_parts(ts);
        assert_eq!((y, m, d), (1899, 12, 29));
    }

    #[test]
    fn format_default_date_alone_at_midnight() {
        assert_eq!(format_default(37623.0), "2003-01-02");
        assert_eq!(format_default(37623.5), "2003-01-02 12:00:00");
        assert_eq!(format_default(-1.1), "1899-12-29 02:24:00");
    }

    #[test]
    fn negative_value_with_time() {
        // Before the epoch the time of day is the absolute value of the
        // fraction, as Access's CDate shows it.
        assert_eq!(timestamp_to_parts(-1.25), (1899, 12, 29, 6, 0, 0));
        assert_eq!(timestamp_to_parts(-0.5), (1899, 12, 30, 12, 0, 0));
        assert_eq!(timestamp_to_parts(-1.1), (1899, 12, 29, 2, 24, 0));
        assert_eq!(timestamp_to_parts(-94.430034345), (1899, 9, 27, 10, 19, 15));
        assert_eq!(timestamp_to_parts(-0.00035134), (1899, 12, 30, 0, 0, 30));
    }

    #[test]
    fn negative_value_rounded_up_to_the_next_date() {
        // 23:59:59.9 on 1899-12-29 rounds to midnight of 1899-12-30.
        let ts = -(1.0 + (86_399.9 / 86_400.0));
        assert_eq!(timestamp_to_parts(ts), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn is_date_only_true() {
        assert!(is_date_only(37623.0));
    }

    #[test]
    fn is_date_only_false() {
        assert!(!is_date_only(37623.5));
    }

    #[test]
    fn is_date_only_epsilon() {
        assert!(is_date_only(37623.0 + 1e-12));
    }

    #[test]
    fn format_year() {
        let s = format_timestamp(37623.0, "%Y");
        assert_eq!(s, "2003");
    }

    #[test]
    fn format_month() {
        let s = format_timestamp(37623.0, "%m");
        assert_eq!(s, "01");
    }

    #[test]
    fn format_day() {
        let s = format_timestamp(37623.0, "%d");
        assert_eq!(s, "02");
    }

    #[test]
    fn format_hour() {
        let s = format_timestamp(37623.5, "%H");
        assert_eq!(s, "12");
    }

    #[test]
    fn format_minute() {
        // 37623 + 30min/(24*60) = 37623 + 0.020833...
        let ts = 37623.0 + 30.0 / 1440.0;
        let s = format_timestamp(ts, "%M");
        assert_eq!(s, "30");
    }

    #[test]
    fn format_second() {
        // 37623 + 45sec/86400
        let ts = 37623.0 + 45.0 / 86400.0;
        let s = format_timestamp(ts, "%S");
        assert_eq!(s, "45");
    }

    #[test]
    fn format_percent_literal() {
        let s = format_timestamp(37623.0, "%%");
        assert_eq!(s, "%");
    }

    #[test]
    fn format_custom_dmy() {
        let s = format_timestamp(37623.0, "%d/%m/%Y");
        assert_eq!(s, "02/01/2003");
    }

    #[test]
    fn format_full_datetime() {
        let ts = 37623.5;
        let s = format_timestamp(ts, "%Y-%m-%d %H:%M:%S");
        assert_eq!(s, "2003-01-02 12:00:00");
    }

    #[test]
    fn rounded_midnight_carries_to_next_day() {
        let ts = 2.0 + 86_399.6 / 86_400.0;
        assert_eq!(timestamp_to_parts(ts), (1900, 1, 2, 0, 0, 0));
    }

    #[test]
    fn day_one() {
        // Day 1 = 1899-12-31
        let (y, m, d, _, _, _) = timestamp_to_parts(1.0);
        assert_eq!((y, m, d), (1899, 12, 31));
    }

    #[test]
    fn day_two() {
        // Day 2 = 1900-01-01
        let (y, m, d, _, _, _) = timestamp_to_parts(2.0);
        assert_eq!((y, m, d), (1900, 1, 1));
    }

    #[test]
    fn nan_returns_epoch() {
        assert_eq!(timestamp_to_parts(f64::NAN), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn infinity_returns_epoch() {
        assert_eq!(timestamp_to_parts(f64::INFINITY), (1899, 12, 30, 0, 0, 0));
    }

    #[test]
    fn neg_infinity_returns_epoch() {
        assert_eq!(
            timestamp_to_parts(f64::NEG_INFINITY),
            (1899, 12, 30, 0, 0, 0)
        );
    }

    #[test]
    fn is_date_only_nan() {
        assert!(is_date_only(f64::NAN));
    }

    #[test]
    fn is_date_only_infinity() {
        assert!(is_date_only(f64::INFINITY));
    }

    #[test]
    fn format_unknown_specifier() {
        let s = format_timestamp(37623.0, "%Z");
        assert_eq!(s, "%Z");
    }

    #[test]
    fn format_trailing_percent() {
        let s = format_timestamp(37623.0, "end%");
        assert!(s.ends_with('%'));
    }

    #[test]
    fn format_no_specifiers() {
        let s = format_timestamp(37623.0, "plain text");
        assert_eq!(s, "plain text");
    }

    #[test]
    fn format_empty_string() {
        let s = format_timestamp(37623.0, "");
        assert_eq!(s, "");
    }

    // -- format_ext_datetime ---------------------------------------------------

    #[test]
    fn ext_datetime_date_only() {
        // 2020-06-17
        assert_eq!(format_ext_datetime(737592, 0, 0), "2020-06-17");
    }

    #[test]
    fn ext_datetime_with_time_and_nanos() {
        // 2021-06-14 22:45:12.3456789
        assert_eq!(
            format_ext_datetime(737954, 81912, 3456789),
            "2021-06-14 22:45:12.3456789"
        );
    }

    #[test]
    fn ext_datetime_epoch() {
        assert_eq!(format_ext_datetime(0, 0, 0), "0001-01-01");
    }

    #[test]
    fn ext_datetime_time_no_nanos() {
        // 2021-06-14 12:45:00
        assert_eq!(format_ext_datetime(737954, 45900, 0), "2021-06-14 12:45:00");
    }
}
