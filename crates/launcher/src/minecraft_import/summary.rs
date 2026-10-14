//! Readable counts for the validated import; internal keys never enter the preview.

use std::collections::BTreeMap;

use super::options::{MAPPINGS, ParsedOptions};
use crate::menu::settings_options::{EXTRA_KEYS, KEY_BINDINGS};

/// The settings groups shown in the import preview, in presentation order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Category {
    Audio,
    Video,
    Controls,
    General,
    Other,
}

impl Category {
    /// Returns the readable settings group name, including the fallback for future keys.
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Controls => "Controls",
            Self::General => "General",
            Self::Other => "Other",
        }
    }
}

/// Classifies a destination using the import registry and accepted keyboard controls.
pub fn category(key: &str) -> Category {
    if let Some(mapping) = MAPPINGS
        .iter()
        .find(|mapping| mapping.destination.name() == key)
    {
        mapping.category
    } else if KEY_BINDINGS.iter().any(|(_, name)| *name == key)
        || EXTRA_KEYS.iter().any(|(name, _)| *name == key)
    {
        Category::Controls
    } else {
        match key {
            "full_screen" => Category::Video,
            "language" => Category::General,
            _ => Category::Other,
        }
    }
}

impl ParsedOptions {
    /// Counts every accepted setting and binding without exposing its storage key.
    pub fn count(&self) -> usize {
        self.values.len()
            + self.bindings.len()
            + usize::from(self.language.is_some())
            + usize::from(self.fullscreen.is_some())
    }

    /// Summarizes each nonempty group, keeping key bindings separate from control settings.
    pub fn summary(&self) -> String {
        let mut groups = BTreeMap::<Category, (usize, usize)>::new();
        for key in self
            .values
            .keys()
            .map(String::as_str)
            .chain(self.language.as_ref().map(|_| "language"))
            .chain(self.fullscreen.map(|_| "full_screen"))
        {
            groups.entry(category(key)).or_default().0 += 1;
        }
        for key in self.bindings.keys() {
            groups.entry(category(key)).or_default().1 += 1;
        }
        if groups.is_empty() {
            return "None".to_owned();
        }
        groups
            .into_iter()
            .map(|(category, (settings, bindings))| {
                let mut counts = Vec::new();
                if settings != 0 {
                    let noun = if category == Category::Audio {
                        "volume setting"
                    } else {
                        "setting"
                    };
                    counts.push(count(settings, noun));
                }
                if bindings != 0 {
                    counts.push(count(bindings, "key binding"));
                }
                format!("{}: {}", category.label(), counts.join(", "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Formats a count with the singular or plural form of a readable noun.
fn count(value: usize, noun: &str) -> String {
    format!("{value} {noun}{}", if value == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
