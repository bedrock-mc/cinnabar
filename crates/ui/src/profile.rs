//! Profile route state shared by input and presentation.

/// The two routes of the Bedrock Profile screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProfileTab {
    #[default]
    Overview,
    Stats,
}

/// Selects the default banner as Bedrock's `mZ` does, using JavaScript UTF-16 units.
pub fn profile_banner_index(identifier: &str, banner_count: usize) -> Option<usize> {
    if banner_count == 0 {
        return None;
    }
    Some(
        identifier
            .encode_utf16()
            .fold(0, |sum, unit| (sum + usize::from(unit)) % banner_count),
    )
}

/// Formats the English abbreviated day/hour/minute duration used by Profile Stats.
/// Bedrock truncates the service's minute value before passing it to DateHelper.
pub fn profile_minutes_display(value: &str) -> Option<String> {
    let minutes = profile_value(value)?;
    if minutes > f64::from(i32::MAX) {
        return None;
    }
    let minutes = minutes as u64;
    let days = minutes / (24 * 60);
    let hours = minutes / 60 % 24;
    let minutes = minutes % 60;
    Some(if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    })
}

/// Formats the integer, comma-grouped value used for non-time Profile statistics.
pub fn profile_count_display(value: &str) -> Option<String> {
    let value = profile_value(value)?;
    if value >= u64::MAX as f64 {
        return None;
    }
    let digits = (value as u64).to_string();
    let mut display = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            display.push(',');
        }
        display.push(digit);
    }
    Some(display)
}

/// Reads a usable service number without displaying missing or invalid data as zero.
fn profile_value(value: &str) -> Option<f64> {
    let value: f64 = value.parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_banner_matches_javascript_character_codes() {
        assert_eq!(profile_banner_index("", 8), Some(0));
        assert_eq!(profile_banner_index("123", 8), Some(6));
        // JavaScript counts both surrogate units, rather than a Unicode scalar.
        assert_eq!(profile_banner_index("😀", 7), Some((0xd83d + 0xde00) % 7));
        assert_eq!(profile_banner_index("123", 0), None);
    }

    #[test]
    fn duration_retains_trailing_fields_and_truncates_service_minutes() {
        for (raw, expected) in [
            ("0", "0m"),
            ("59.9", "59m"),
            ("60", "1h 0m"),
            ("120.5", "2h 0m"),
            ("1439", "23h 59m"),
            ("1440", "1d 0h 0m"),
            ("1501", "1d 1h 1m"),
        ] {
            assert_eq!(profile_minutes_display(raw).as_deref(), Some(expected));
        }
    }

    #[test]
    fn counts_and_distance_use_grouped_raw_integers() {
        assert_eq!(profile_count_display("0").as_deref(), Some("0"));
        assert_eq!(profile_count_display("12345.75").as_deref(), Some("12,345"));
        assert_eq!(
            profile_count_display("1234567").as_deref(),
            Some("1,234,567")
        );
        for invalid in ["", "-1", "NaN", "Infinity", "1e100"] {
            assert_eq!(profile_count_display(invalid), None);
            assert_eq!(profile_minutes_display(invalid), None);
        }
    }
}
