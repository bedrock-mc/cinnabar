//! Native three-sampler rasters share one array page. Nearest texel replication preserves
//! each source's normalized-UV lookup; RGB composition remains in the fragment shader.

use super::*;

pub(super) fn page_dimensions(catalog: &RuntimeActorCatalog) -> (u16, u16) {
    catalog
        .textures()
        .iter()
        .enumerate()
        .filter(|(index, _)| catalog.texture_uses_multitexture(*index))
        .fold((0, 0), |(width, height), (_, texture)| {
            (width.max(texture.width), height.max(texture.height))
        })
}

pub(super) fn page_pixels(
    catalog: &RuntimeActorCatalog,
    indices: &[usize],
    width: u16,
    height: u16,
) -> Vec<u8> {
    let mut pixels =
        Vec::with_capacity(usize::from(width) * usize::from(height) * 4 * indices.len());
    for &index in indices {
        let texture = &catalog.textures()[index];
        append_nearest(&mut pixels, texture, width, height);
    }
    pixels
}

fn append_nearest(pixels: &mut Vec<u8>, texture: &assets::ActorTexture, width: u16, height: u16) {
    if (texture.width, texture.height) == (width, height) {
        pixels.extend_from_slice(&texture.rgba8);
        return;
    }
    // Witnessed native dimensions are integer subdivisions of their shared page. Never filter
    // the tiny alpha masks or interpolate art into a different texture.
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            let source_x = x * usize::from(texture.width) / usize::from(width);
            let source_y = y * usize::from(texture.height) / usize::from(height);
            let at = (source_y * usize::from(texture.width) + source_x) * 4;
            pixels.extend_from_slice(&texture.rgba8[at..at + 4]);
        }
    }
}

impl ActorArtworkPages {
    /// The controller's three samplers, routed together without drawing the same mesh thrice.
    pub fn multitexture_location(
        &self,
        rig: EntityRigId,
        sources: [u32; 3],
    ) -> Option<ActorArtworkLocation> {
        let mut base = self.variant_location(rig, sources[0])?;
        let second = self.variant_location(rig, sources[1])?;
        let third = self.variant_location(rig, sources[2])?;
        if base.page != second.page || base.page != third.page {
            return None;
        }
        let page = self.pages.get(usize::from(base.page).checked_sub(1)?)?;
        if !page.multitexture {
            return None;
        }
        base.multitexture = Some([second.layer, third.layer]);
        Some(base)
    }

    pub(super) fn valid_multitexture(&self, location: ActorArtworkLocation) -> bool {
        let Some(layers) = location.multitexture else {
            return true;
        };
        let Some(page) = usize::from(location.page)
            .checked_sub(1)
            .and_then(|index| self.pages.get(index))
        else {
            return false;
        };
        page.multitexture
            && layers.iter().all(|&layer| {
                layer < page.layers && self.entity_locations.contains(&(location.page, layer))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_expansion_preserves_partial_alpha_and_normalized_texel_boundaries() {
        let texture = assets::ActorTexture {
            source: 0,
            width: 2,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: Arc::from([10, 20, 30, 2, 40, 50, 60, 255]),
        };
        let mut pixels = Vec::new();
        append_nearest(&mut pixels, &texture, 4, 2);
        assert_eq!(pixels.len(), 4 * 2 * 4);
        for (index, pixel) in pixels.chunks_exact(4).enumerate() {
            let source = (index % 4) / 2;
            assert_eq!(pixel, &texture.rgba8[source * 4..source * 4 + 4]);
        }
    }

    #[test]
    fn grouped_texture_route_requires_native_material_and_same_page() {
        let location = |page, layer| ActorArtworkLocation {
            page,
            layer,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        };
        let first = location(1, 0);
        let pages = ActorArtworkPages {
            routes: Arc::new(BTreeMap::from([(EntityRigId(0), first)])),
            source_locations: Arc::new(BTreeMap::from([
                (5, first),
                (6, location(1, 1)),
                (7, location(1, 2)),
                (8, location(2, 0)),
            ])),
            entity_locations: Arc::new(BTreeSet::from([(1, 0), (1, 1), (1, 2), (2, 0)])),
            pages: Arc::from([ActorTexturePage {
                width: 1,
                height: 1,
                layers: 3,
                rgba8: Arc::from([0; 12]),
                color_mask: false,
                multitexture: true,
            }]),
            ..Default::default()
        };
        let route = pages
            .multitexture_location(EntityRigId(0), [5, 6, 7])
            .unwrap();
        assert_eq!(route.multitexture, Some([1, 2]));
        assert!(pages.valid(EntityRigId(0), route));
        assert!(
            pages
                .multitexture_location(EntityRigId(0), [5, 6, 8])
                .is_none()
        );
        assert!(!pages.valid(
            EntityRigId(0),
            ActorArtworkLocation {
                multitexture: Some([1, 3]),
                ..route
            }
        ));
        let mut ordinary = pages.clone();
        Arc::make_mut(&mut ordinary.pages)[0].multitexture = false;
        assert!(
            ordinary
                .multitexture_location(EntityRigId(0), [5, 6, 7])
                .is_none()
        );
    }
}
