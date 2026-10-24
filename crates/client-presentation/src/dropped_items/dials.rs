//! Keeps active dial frames resident so animation only changes instance model handles.

use std::{collections::BTreeMap, ops::Range};

use super::{ModelCache, ModelKey, UiPresentationRuntime, icon_sprite};

/// The distinct animation strips used by dropped items in this frame.
#[derive(Default)]
pub(super) struct DialModels {
    strips: BTreeMap<&'static str, Range<u32>>,
}

impl DialModels {
    /// Collects each active strip once, leaving static and session icons alone.
    pub(super) fn observe(
        &mut self,
        icons: &UiPresentationRuntime,
        identifier: &str,
        metadata: u32,
    ) {
        if let Some((identifier, frames)) = icons.dial_icon_frames(identifier, metadata) {
            self.strips.entry(identifier).or_insert(frames);
        }
    }

    /// Caches active frames after current models are resolved; publication limits extra GPU frames.
    pub(super) fn warm(&self, cache: &mut ModelCache, icons: &UiPresentationRuntime) {
        for (&identifier, frames) in &self.strips {
            for metadata in frames.clone() {
                if let Some(icon) = icons.item_icon(identifier, metadata) {
                    cache.warm_icon(
                        ModelKey::icon(icon, true),
                        || super::source_cost::icon_cost(icons, icon, true),
                        || icon_sprite(icons, identifier, metadata, true),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
