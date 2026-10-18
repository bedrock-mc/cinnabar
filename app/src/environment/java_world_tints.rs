//! Applies optional biome colours without changing the dense world biome indices.

use assets::{
    ResolvedBiomeTints,
    java_world::{BIRCH_COLOUR, EVERGREEN_COLOUR},
};
use render_scene::{BiomeTint, ChunkBiomeTints, JavaWorldVisuals};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct JavaTintState {
    native: Option<ChunkBiomeTints>,
    enabled: Option<bool>,
    published: Option<Arc<[BiomeTint]>>,
}

impl JavaTintState {
    /// Rebuilds only after a style or native table change, preserving authored field replacements.
    pub(crate) fn update(
        &mut self,
        java: &JavaWorldVisuals,
        resolved: &ResolvedBiomeTints,
        active: &mut ChunkBiomeTints,
    ) {
        if self
            .native
            .as_ref()
            .is_none_or(|native| native.table_identity() != active.table_identity())
            || self
                .published
                .as_ref()
                .is_some_and(|published| !Arc::ptr_eq(published, &active.entries))
        {
            self.native = Some(active.clone());
            self.enabled = None;
        }
        if self.enabled == Some(java.enabled()) {
            return;
        }
        let native = self.native.as_ref().expect("native tint table captured");
        self.enabled = Some(java.enabled());
        let Some(assets) = java.assets() else {
            *active = native.clone();
            self.published = Some(Arc::clone(&active.entries));
            return;
        };
        let mut entries = native.entries().to_vec();
        for (index, biome) in resolved.biomes.iter().enumerate() {
            let Some(identifier) = biome.as_ref().map(|biome| biome.identifier.as_ref()) else {
                continue;
            };
            let Some((base, colours)) = java
                .base_biome_tint(identifier)
                .zip(assets.biome_colours(identifier))
            else {
                continue;
            };
            let Some(tint) = entries.get_mut(index) else {
                continue;
            };
            replace(&mut tint.grass, base.grass, colours.grass);
            replace(&mut tint.foliage, base.foliage, colours.foliage);
            replace(&mut tint.dry_foliage, base.dry_foliage, colours.dry_foliage);
            replace(&mut tint.birch, base.birch, BIRCH_COLOUR);
            replace(&mut tint.evergreen, base.evergreen, EVERGREEN_COLOUR);
            replace(&mut tint.water, base.water, colours.water);
            if tint.water_opacity == base.water[3] {
                tint.water_opacity = 1.0;
            }
            if tint.foliage == linear(colours.foliage) {
                tint.flags &= !assets::BIOME_TINT_FLAG_SEASONAL_FOLIAGE;
            }
        }
        *active = ChunkBiomeTints::with_identity(Arc::from(entries), native.table_identity());
        self.published = Some(Arc::clone(&active.entries));
    }
}

/// Replaces a base field while retaining an independently authored server value.
fn replace(active: &mut [f32; 3], base: [f32; 4], java: u32) {
    if *active == [base[0], base[1], base[2]] {
        *active = linear(java);
    }
}

/// Converts an encoded RGB colour at the renderer's existing tint boundary.
fn linear(rgb: u32) -> [f32; 3] {
    render_model::celestial::rgb_to_linear([
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_colour_fields_keep_priority_over_the_style() {
        let base = [0.25, 0.5, 0.75, 1.0];
        let mut original = [0.25, 0.5, 0.75];
        replace(&mut original, base, 0xffffff);
        assert_eq!(original, [1.0; 3]);
        let mut authored = [0.4, 0.3, 0.2];
        replace(&mut authored, base, 0xffffff);
        assert_eq!(authored, [0.4, 0.3, 0.2]);
    }
}
