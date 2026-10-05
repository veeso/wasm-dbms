use crate::prelude::{DateTime, DbmsError, DbmsResult, Sanitize, Value};

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
                validate_calendar_fields(&dt)?;

                let delta_minutes = self.0 - dt.timezone_offset_minutes;
                let delta_us = delta_minutes as i64 * 60 * 1_000_000;

                let ts = datetime_to_us(&dt) + delta_us;
                let mut new_dt = us_to_datetime(ts)?;

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

/// Checks that every calendar and clock field of `dt` names an existing instant.
fn validate_calendar_fields(dt: &DateTime) -> DbmsResult<()> {
    let date_is_valid = (1..=12).contains(&dt.month) && dt.day >= 1 && {
        // A valid date is the only one that survives the round trip through a day count.
        let days = days_from_civil(dt.year.into(), dt.month.into(), dt.day.into());
        civil_from_days(days) == (dt.year.into(), dt.month, dt.day)
    };
    let time_is_valid =
        dt.hour < 24 && dt.minute < 60 && dt.second < 60 && dt.microsecond < 1_000_000;

    if date_is_valid && time_is_valid {
        Ok(())
    } else {
        Err(DbmsError::Sanitize(format!(
            "invalid date time fields: {dt:?}"
        )))
    }
}

fn us_to_datetime(mut ts: i64) -> DbmsResult<DateTime> {
    let microsecond = (ts.rem_euclid(1_000_000)) as u32;
    ts = ts.div_euclid(1_000_000);

    let second = (ts.rem_euclid(60)) as u8;
    ts = ts.div_euclid(60);

    let minute = (ts.rem_euclid(60)) as u8;
    ts = ts.div_euclid(60);

    let hour = (ts.rem_euclid(24)) as u8;
    let days = ts.div_euclid(24);

    let (year, month, day) = civil_from_days(days);
    let year = u16::try_from(year).map_err(|_| {
        DbmsError::Sanitize(format!(
            "converted year {year} is outside the supported range 0..={}",
            u16::MAX
        ))
    })?;

    Ok(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
        timezone_offset_minutes: 0,
    })
}

fn datetime_to_us(dt: &DateTime) -> i64 {
    let days = days_from_civil(dt.year.into(), dt.month.into(), dt.day.into());

    let seconds = days * 86_400 + dt.hour as i64 * 3_600 + dt.minute as i64 * 60 + dt.second as i64;

    seconds * 1_000_000 + dt.microsecond as i64
}

/// Returns the number of days from 1970-01-01 to the given proleptic Gregorian date.
///
/// The result is negative for dates before the Unix epoch.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Count years from March, so that the leap day is the last day of the year.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

/// Returns the proleptic Gregorian `(year, month, day)` for the given number of days from
/// 1970-01-01, which is negative for dates before the Unix epoch.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400;
    let year = if month <= 2 { year + 1 } else { year };

    (year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::DbmsError;

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

    #[test]
    fn test_should_shift_pre_epoch_datetime() {
        let sanitizer = TimezoneSanitizer(60);

        let input = dt(1969, 12, 31, 12, 0, 0, 0, 0);
        let expected = dt(1969, 12, 31, 13, 0, 0, 0, 60);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_underflow_across_epoch() {
        let sanitizer = UtcSanitizer;

        let input = dt(1970, 1, 1, 0, 0, 0, 0, 60);
        let expected = dt(1969, 12, 31, 23, 0, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_overflow_across_epoch() {
        let sanitizer = TimezoneSanitizer(60);

        let input = dt(1969, 12, 31, 23, 30, 0, 0, 0);
        let expected = dt(1970, 1, 1, 0, 30, 0, 0, 60);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_shift_across_non_leap_century_before_epoch() {
        let sanitizer = UtcSanitizer;

        let input = dt(1900, 3, 1, 0, 30, 0, 0, 60);
        let expected = dt(1900, 2, 28, 23, 30, 0, 0, 0);

        let out = sanitizer.sanitize(Value::DateTime(input)).unwrap();

        assert_eq!(out, Value::DateTime(expected));
    }

    #[test]
    fn test_should_roundtrip_pre_epoch_conversion() {
        let dt0 = dt(1601, 1, 1, 0, 0, 0, 1, 0);

        let v1 = TimezoneSanitizer(-300)
            .sanitize(Value::DateTime(dt0))
            .unwrap();
        assert_eq!(v1, Value::DateTime(dt(1600, 12, 31, 19, 0, 0, 1, -300)));

        let v2 = UtcSanitizer.sanitize(v1).unwrap();
        assert_eq!(v2, Value::DateTime(dt0));
    }

    #[test]
    fn test_should_reject_result_before_year_zero() {
        let sanitizer = UtcSanitizer;

        let input = dt(0, 1, 1, 0, 0, 0, 0, 60);

        let err = sanitizer.sanitize(Value::DateTime(input)).unwrap_err();

        assert!(matches!(err, DbmsError::Sanitize(_)), "{err:?}");
    }

    #[test]
    fn test_should_reject_result_after_year_65535() {
        let sanitizer = TimezoneSanitizer(60);

        let input = dt(u16::MAX, 12, 31, 23, 30, 0, 0, 0);

        let err = sanitizer.sanitize(Value::DateTime(input)).unwrap_err();

        assert!(matches!(err, DbmsError::Sanitize(_)), "{err:?}");
    }

    #[test]
    fn test_should_shift_at_supported_year_bounds() {
        let first = dt(0, 1, 1, 1, 30, 0, 0, 60);
        assert_eq!(
            UtcSanitizer.sanitize(Value::DateTime(first)).unwrap(),
            Value::DateTime(dt(0, 1, 1, 0, 30, 0, 0, 0))
        );

        let last = dt(u16::MAX, 12, 31, 22, 30, 0, 0, 0);
        assert_eq!(
            TimezoneSanitizer(60)
                .sanitize(Value::DateTime(last))
                .unwrap(),
            Value::DateTime(dt(u16::MAX, 12, 31, 23, 30, 0, 0, 60))
        );
    }

    #[test]
    fn test_should_reject_invalid_calendar_fields() {
        let invalid = [
            dt(2025, 0, 1, 0, 0, 0, 0, 0),
            dt(2025, 13, 1, 0, 0, 0, 0, 0),
            dt(2025, 14, 1, 0, 0, 0, 0, 0),
            dt(2025, u8::MAX, 1, 0, 0, 0, 0, 0),
            dt(2025, 1, 0, 0, 0, 0, 0, 0),
            dt(2025, 1, 32, 0, 0, 0, 0, 0),
            dt(2025, 4, 31, 0, 0, 0, 0, 0),
            dt(2025, 2, 29, 0, 0, 0, 0, 0),
            dt(1900, 2, 29, 0, 0, 0, 0, 0),
            dt(2025, 2, 30, 0, 0, 0, 0, 0),
            dt(2025, 1, 1, 24, 0, 0, 0, 0),
            dt(2025, 1, 1, 0, 60, 0, 0, 0),
            dt(2025, 1, 1, 0, 0, 60, 0, 0),
            dt(2025, 1, 1, 0, 0, 0, 1_000_000, 0),
        ];

        for input in invalid {
            for sanitizer in [TimezoneSanitizer(0), TimezoneSanitizer(60)] {
                let outcome =
                    std::panic::catch_unwind(|| sanitizer.sanitize(Value::DateTime(input)));
                let result = outcome.unwrap_or_else(|_| panic!("sanitizer panicked on {input:?}"));
                let err = result.expect_err(&format!("{input:?} should be rejected"));
                assert!(matches!(err, DbmsError::Sanitize(_)), "{err:?}");
            }
        }
    }

    #[test]
    fn test_should_accept_last_valid_calendar_fields() {
        let input = dt(2000, 2, 29, 23, 59, 59, 999_999, 0);

        let out = TimezoneSanitizer(0)
            .sanitize(Value::DateTime(input))
            .unwrap();

        assert_eq!(out, Value::DateTime(input));
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
