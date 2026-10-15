//! UTC `yyyymmdd-hhmmss` stamps for folder names, from caller-supplied Unix milliseconds.

const SECONDS_PER_DAY: u64 = 86_400;
/// Length of `yyyymmdd-hhmmss`.
pub(crate) const STAMP_LEN: usize = 15;

/// Formats the UTC second containing `unix_ms`.
pub(crate) fn stamp(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000;
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    let time = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

/// Formats `unix_ms` as `yyyy-mm-ddThh:mm:ss.mmmZ`.
pub(crate) fn iso_ms(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000;
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    let time = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        time / 3600,
        time / 60 % 60,
        time % 60,
        unix_ms % 1000
    )
}

/// Parses a stamp back to the Unix milliseconds of its first instant.
pub(crate) fn parse(stamp: &str) -> Option<u64> {
    let bytes = stamp.as_bytes();
    if bytes.len() != STAMP_LEN
        || bytes[8] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| stamp[range].parse::<u64>().ok();
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(9..11)?, number(11..13)?, number(13..15)?);
    if year < 1970
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * SECONDS_PER_DAY) + hour * 3600 + minute * 60 + second) * 1000)
}

/// Days in `month` (1-12) of the proleptic Gregorian `year`, counting leap-year Februaries.
fn days_in_month(year: u64, month: u64) -> u64 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Converts days since 1970-01-01 to a proleptic Gregorian date (Hinnant's algorithm).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Inverse of [`civil_from_days`] for dates from 1970 onward.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year % 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_seconds() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(999), "19700101-000000");
        assert_eq!(stamp(1_000_000_000_000), "20010909-014640");
        assert_eq!(stamp(951_782_400_000), "20000229-000000");
        assert_eq!(stamp(4_102_444_799_000), "20991231-235959");
    }

    #[test]
    fn stamps_parse_back_to_their_second() {
        for unix_ms in [0, 951_782_400_000, 1_000_000_000_123, 1_760_443_200_999] {
            assert_eq!(parse(&stamp(unix_ms)), Some(unix_ms / 1000 * 1000));
        }
        for invalid in [
            "20261014-1200",
            "20261014_120000",
            "2026101a-120000",
            "20261314-120000",
            "20250229-120000",
            "20261014-240000",
            "19691231-235959",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }
}
