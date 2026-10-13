//! Converts supported saved desktop key codes to the client's physical controls.

use semantic_input::PhysicalControl;

/// Translates common desktop virtual keys and mouse buttons; unknown encodings are skipped.
pub(super) fn control(raw: &str) -> Option<Option<PhysicalControl>> {
    let key = raw.parse::<i32>().ok()?;
    let usage = match key {
        0 => return Some(None),
        -99..=-92 => return Some(Some(PhysicalControl::MouseButton((key + 100) as u8))),
        65..=90 => (key - 65 + 0x04) as u16,
        49..=57 => (key - 49 + 0x1e) as u16,
        48 => 0x27,
        112..=123 => (key - 112 + 0x3a) as u16,
        8 => 0x2a,
        9 => 0x2b,
        13 => 0x28,
        16 => 0xe1,
        17 => 0xe0,
        18 => 0xe2,
        27 => 0x29,
        32 => 0x2c,
        37 => 0x50,
        38 => 0x52,
        39 => 0x4f,
        40 => 0x51,
        46 => 0x4c,
        _ => return None,
    };
    Some(Some(PhysicalControl::KeyboardUsage(usage)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_keyboard_codes_are_not_treated_as_usb_usages() {
        assert_eq!(
            control("87"),
            Some(Some(PhysicalControl::KeyboardUsage(0x1a)))
        );
        assert_eq!(control("-99"), Some(Some(PhysicalControl::MouseButton(1))));
        assert_eq!(control("0"), Some(None));
        assert_eq!(control("99999"), None);
    }
}
