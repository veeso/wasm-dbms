use crate::prelude::{DateTime, DbmsResult, Sanitize, Value};

/// Sanitizer that ensures that all [`crate::prelude::DateTime`] values are within a specific timezone.
///
/// If you want to ensure that all datetime values are in UTC timezone, you can use directly the [`UtcSanitizer`],
/// which actually is just a wrapper for this sanitizer with "UTC" as timezone.
///
/// The value provided is `i16` representing the timezone offset in minutes from UTC.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{CollapseWhitespaceSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("  Hello,       World!  ".into());
/// let sanitizer = CollapseWhitespaceSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("Hello, World!".into()));
/// ```
pub struct TimezoneSanitizer(pub i16);

impl Sanitize for TimezoneSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::DateTime(dt) => {
                let delta_minutes = self.0 - dt.timezone_offset_minutes;
                let delta_us = delta_minutes as i64 * 60 * 1_000_000;

                let ts = datetime_to_us(&dt) + delta_us;
                let mut new_dt = us_to_datetime(ts);

                new_dt.timezone_offset_minutes = self.0;

                Ok(Value::DateTime(new_dt))
            }
            other => Ok(other),
        }
    }
}

/// Sanitizer that ensures that all [`crate::prelude::DateTime`] values are within the UTC timezone.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{CollapseWhitespaceSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("  Hello,       World!  ".into());
/// let sanitizer = CollapseWhitespaceSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("Hello, World!".into()));
/// ```
pub struct UtcSanitizer;

impl Sanitize for UtcSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        TimezoneSanitizer(0).sanitize(value)
    }
}

fn us_to_datetime(mut ts: i64) -> DateTime {
    let microsecond = (ts.rem_euclid(1_000_000)) as u32;
    ts = ts.div_euclid(1_000_000);

    let second = (ts.rem_euclid(60)) as u8;
    ts = ts.div_euclid(60);

    let minute = (ts.rem_euclid(60)) as u8;
    ts = ts.div_euclid(60);

    let hour = (ts.rem_euclid(24)) as u8;
    let mut days = ts.div_euclid(24);

    let mut year = 1970;
    loop {
        let yd = if is_leap(year) { 366 } else { 365 };
        if days >= yd {
            days -= yd;
            year += 1;
        } else {
            break;
        }
    }

    let mut month = 1;
    loop {
        let dim = days_in_month(year, month);
        if days >= dim as i64 {
            days -= dim as i64;
            month += 1;
        } else {
            break;
        }
    }

    let day = (days + 1) as u8;

    DateTime {
        year: year as u16,
        month: month as u8,
        day,
        hour,
        minute,
        second,
        microsecond,
        timezone_offset_minutes: 0,
    }
}

fn datetime_to_us(dt: &DateTime) -> i64 {
    let mut days = 0i64;

    for y in 1970..dt.year as i32 {
        days += if is_leap(y) { 366 } else { 365 };
    }

    for m in 1..dt.month as i32 {
        days += days_in_month(dt.year as i32, m) as i64;
    }

    days += (dt.day as i64) - 1;

    let seconds = days * 86_400 + dt.hour as i64 * 3_600 + dt.minute as i64 * 60 + dt.second as i64;

    seconds * 1_000_000 + dt.microsecond as i64
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 => 31,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_noop_timezone_if_same_offset() {
        let sanitizer = TimezoneSanitizer(120);

        let original = dt(2024, 3, 10, 12, 30, 0, 0, 120);
        let value = Value::DateTime(original);

        let out = sanitizer.sanitize(value).unwrap();

        assert_eq!(out, Value::DateTime(original));
    }

    #[test]
    fn test_should_shift_one_hour_forward() {
        let sanitizer = TimezoneSanitizer(120);

        let input = dt(2024, 3, 10, 12, 0, 0, 0, 60);
        let expected = dt(2024, 3, 10, 13, 0, 0, 0, 120);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_shift_one_hour_backward_with_day_underflow() {
        let sanitizer = UtcSanitizer;

        let input = dt(2024, 3, 10, 0, 30, 0, 0, 60);
        let expected = dt(2024, 3, 9, 23, 30, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_underflow_across_month_boundary() {
        let sanitizer = TimezoneSanitizer(0);

        let input = dt(2024, 4, 1, 0, 15, 0, 0, 60);
        let expected = dt(2024, 3, 31, 23, 15, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_underflow_year_boundary() {
        let sanitizer = TimezoneSanitizer(0);

        let input = dt(2024, 1, 1, 0, 0, 0, 0, 60);
        let expected = dt(2023, 12, 31, 23, 0, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_shift_leap_day() {
        let sanitizer = TimezoneSanitizer(0);

        let input = dt(2024, 2, 29, 0, 30, 0, 0, 60);
        let expected = dt(2024, 2, 28, 23, 30, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_preserve_microseconds() {
        let sanitizer = TimezoneSanitizer(60);

        let input = dt(2024, 5, 20, 10, 0, 0, 999_999, 0);
        let expected = dt(2024, 5, 20, 11, 0, 0, 999_999, 60);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_timezone_sanitizer_noop_on_non_datetime() {
        let sanitizer = TimezoneSanitizer(60);

        let value = Value::Int32(42.into());
        let out = sanitizer.sanitize(value.clone()).unwrap();

        assert_eq!(out, value);
    }

    #[test]
    fn test_should_roundtrip_conversion() {
        let dt0 = dt(2024, 6, 15, 18, 45, 12, 123_456, 0);

        let to_plus2 = TimezoneSanitizer(120);
        let to_utc = UtcSanitizer;

        let v1 = to_plus2.sanitize(Value::DateTime(dt0)).unwrap();

        let v2 = to_utc.sanitize(v1).unwrap();

        assert_eq!(v2, Value::DateTime(dt0));
    }

    #[allow(clippy::too_many_arguments)]
    fn dt(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8, us: u32, tz: i16) -> DateTime {
        DateTime {
            year: y,
            month: mo,
            day: d,
            hour: h,
            minute: mi,
            second: s,
            microsecond: us,
            timezone_offset_minutes: tz,
        }
    }
}
