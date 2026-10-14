//! Driver rules use numeric version ranges and never infer a range from an unknown version.

use super::{Adapter, Backend, Platform};
use serde::Deserialize;

/// One known driver failure; bounds are inclusive and absent bounds are open.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub platform: Platform,
    pub vendor: u32,
    pub device: Option<u32>,
    pub integrated: Option<bool>,
    pub driver_contains: Option<String>,
    pub min_version: Option<Vec<u32>>,
    pub max_version: Option<Vec<u32>>,
    pub backend: Backend,
    pub reason: String,
}

/// Parses the shipped rule file once; invalid tracked rules fail rather than silently disappearing.
pub fn rules() -> &'static [Rule] {
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/graphics-driver-blocklist.json"))
            .expect("tracked graphics driver rules must parse")
    })
}

/// Extracts the first dotted numeric version, allowing prefixes such as `Mesa`.
fn version(text: &str) -> Option<Vec<u32>> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|part| part.contains('.'))
        .find_map(|part| {
            part.split('.')
                .map(str::parse)
                .collect::<Result<Vec<u32>, _>>()
                .ok()
        })
}

/// Compares numeric components with trailing zeros padded, so 10.2 equals 10.2.0.
fn compare(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    (0..a.len().max(b.len()))
        .map(|i| {
            a.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&b.get(i).copied().unwrap_or(0))
        })
        .find(|order| !order.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}

impl Rule {
    /// Matches only the specified platform, hardware, driver and inclusive numeric range.
    pub fn matches(&self, platform: Platform, adapter: &Adapter) -> bool {
        if self.platform != platform
            || self.vendor != adapter.vendor
            || self.device.is_some_and(|device| device != adapter.device)
            || self
                .integrated
                .is_some_and(|integrated| integrated != adapter.integrated)
            || self
                .driver_contains
                .as_ref()
                .is_some_and(|driver| !adapter.driver.contains(driver))
        {
            return false;
        }
        if self.min_version.is_none() && self.max_version.is_none() {
            return true;
        }
        let Some(version) = version(&adapter.driver_version)
            .or_else(|| version(&adapter.driver_info))
            .or_else(|| version(&adapter.driver))
        else {
            return false;
        };
        self.min_version
            .as_ref()
            .is_none_or(|min| !compare(&version, min).is_lt())
            && self
                .max_version
                .as_ref()
                .is_none_or(|max| !compare(&version, max).is_gt())
    }
}
