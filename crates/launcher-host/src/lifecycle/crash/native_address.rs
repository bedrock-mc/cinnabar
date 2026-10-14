//! Address formatting shared by native capture and portable regression tests.

use serde::Serialize;

/// One fault or backtrace address, with module-relative location when it can be resolved.
#[derive(Debug, Serialize)]
pub(super) struct Address {
    pub address: String,
    pub module: Option<String>,
    pub offset: Option<String>,
}

impl Address {
    /// Formats an address without pretending an unresolved module has a valid offset.
    pub(super) fn new(address: u64, module: Option<(String, u64)>) -> Self {
        let location =
            module.and_then(|(name, base)| address.checked_sub(base).map(|offset| (name, offset)));
        Self {
            address: format!("0x{address:X}"),
            module: location.as_ref().map(|(name, _)| name.clone()),
            offset: location.map(|(_, offset)| format!("0x{offset:X}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_location_uses_a_module_relative_hex_offset() {
        let location = Address::new(0x1000_1234, Some(("driver.dll".into(), 0x1000_0000)));
        assert_eq!(location.address, "0x10001234");
        assert_eq!(location.module.as_deref(), Some("driver.dll"));
        assert_eq!(location.offset.as_deref(), Some("0x1234"));
    }

    #[test]
    fn unknown_or_invalid_module_base_has_no_offset() {
        for module in [None, Some(("invalid.dll".into(), 10))] {
            let location = Address::new(1, module);
            assert_eq!(location.address, "0x1");
            assert!(location.module.is_none());
            assert!(location.offset.is_none());
        }
    }
}
