//! Native fault locations recorded in `native` report fields.

use serde::{Deserialize, Serialize};

/// One fault or backtrace address, with its module-relative location when it can be resolved.
///
/// `module` is a file name only, never a path, so reports do not reveal the install location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    pub address: String,
    pub module: Option<String>,
    pub offset: Option<String>,
}

impl Address {
    /// Formats `address`, relative to `module`'s `(file name, base)` when the base precedes it,
    /// without pretending an unresolved module has a valid offset.
    pub fn new(address: u64, module: Option<(String, u64)>) -> Self {
        let location =
            module.and_then(|(name, base)| address.checked_sub(base).map(|offset| (name, offset)));
        Self {
            address: format!("0x{address:X}"),
            module: location.as_ref().map(|(name, _)| name.clone()),
            offset: location.map(|(_, offset)| format!("0x{offset:X}")),
        }
    }

    /// `module+offset`, or the bare address when the module is unknown.
    pub fn location(&self) -> String {
        match (&self.module, &self.offset) {
            (Some(module), Some(offset)) => format!("{module}+{offset}"),
            _ => self.address.clone(),
        }
    }
}

/// The file name of a module path in either separator style.
pub fn module_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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
        assert_eq!(location.location(), "driver.dll+0x1234");
    }

    #[test]
    fn unknown_or_invalid_module_base_has_no_offset() {
        for module in [None, Some(("invalid.dll".into(), 10))] {
            let location = Address::new(1, module);
            assert_eq!(location.address, "0x1");
            assert!(location.module.is_none());
            assert!(location.offset.is_none());
            assert_eq!(location.location(), "0x1");
        }
    }

    #[test]
    fn module_names_drop_every_directory() {
        assert_eq!(
            module_name(r"C:\Users\someone\Cinnabar\cinnabar.exe"),
            "cinnabar.exe"
        );
        assert_eq!(module_name("/home/someone/.local/bin/cinnabar"), "cinnabar");
        assert_eq!(module_name("cinnabar"), "cinnabar");
    }
}
