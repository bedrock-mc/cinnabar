//! Prepared CPU masks use ordinary atlas sprites after all native painters have run.

use crate::{
    atlas::ServerAtlas,
    engine::{Canvas, screen::PaintError},
    textures::Textures,
};
use render_api::sprite_mask::{MaskLayout, SpriteMask};
use render_model::UiTexturePage;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use ui::{UiBlendMode, UiMesh, UiNode, UiVisual};

#[derive(Default)]
pub(crate) struct MaskCache {
    rows: BTreeMap<String, (MaskLayout, Arc<[Tile]>)>,
}

struct Tile {
    image: UiTexturePage,
    bounds: [u32; 4],
}

struct PendingSprite {
    node: usize,
    key: String,
    color: [u8; 4],
    blend: UiBlendMode,
}

#[derive(Default)]
pub(crate) struct PreparedRasters {
    namespaces: BTreeSet<String>,
    rows: BTreeSet<String>,
    images: BTreeMap<String, UiTexturePage>,
    sprites: Vec<PendingSprite>,
}

impl PreparedRasters {
    /// Publishes only each painter's namespace and resolves its already transformed sprite nodes.
    pub(crate) fn publish(
        self,
        atlas: &mut ServerAtlas,
        first: u16,
        nodes: &mut [UiNode],
        set: &crate::textures::TextureSet,
    ) -> Result<(), PaintError> {
        for namespace in self.namespaces {
            set.masks
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .rows
                .retain(|key, _| !key.starts_with(&namespace) || self.rows.contains(key));
            let images = self
                .images
                .iter()
                .filter(|(key, _)| key.starts_with(&namespace))
                .map(|(key, image)| (key.clone(), image.clone()))
                .collect();
            atlas.set_client_rasters(&namespace, &images);
        }
        atlas.require_current(self.images.keys().map(String::as_str));
        for sprite in self.sprites {
            if let Some(slot) = atlas.placement(&sprite.key) {
                let [x, y, w, h] = slot.rect.map(f32::from);
                let mesh = UiMesh::sprite(
                    first + slot.page,
                    [x, y, x + w, y + h],
                    sprite.color,
                    sprite.blend,
                )
                .map_err(|_| PaintError::InvalidFontTexture)?;
                nodes[sprite.node] = nodes[sprite.node]
                    .clone()
                    .with_visual(UiVisual::Mesh(Arc::new(mesh)));
            }
        }
        Ok(())
    }
}

impl Textures<'_> {
    /// Reads the prepared source outline, skipping the overlay until its worker completes.
    pub fn outline_mask(&self, path: &str) -> Option<Arc<SpriteMask>> {
        self.admits(path)
            .then(|| self.atlas.original_outline(&self.canonical(path)))
            .flatten()
    }
}

impl Canvas<'_> {
    /// Chooses mask texels from the final physical scale, including a control's paint transform.
    pub fn raster_size(&self, bounds: [f32; 4]) -> [u32; 2] {
        let scale = self.residual.map_or([1., 1.], |map| {
            let [a, b, c, d] = map.linear;
            let determinant = (a * d - b * c).abs();
            [
                determinant / c.hypot(d).max(f64::MIN_POSITIVE),
                determinant / a.hypot(b).max(f64::MIN_POSITIVE),
            ]
            .map(|scale| scale as f32)
        });
        std::array::from_fn(|axis| {
            raster_axis_size(
                (bounds[axis + 2] - bounds[axis]).abs()
                    * self.metrics.dpi_scale.get()
                    * scale[axis],
            )
        })
    }

    /// Queues one combined mask, split into disjoint ordinary sprites when it spans atlas pages.
    pub fn mask(
        &mut self,
        namespace: &str,
        key: &str,
        layout: MaskLayout,
        bounds: [f32; 4],
        color: [u8; 4],
        blend: UiBlendMode,
    ) -> Result<(), PaintError> {
        let cache_key = format!("{namespace}{key}");
        let mut cache = self
            .textures
            .set
            .masks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let tiles = if let Some((old, tiles)) = cache.rows.get(&cache_key)
            && *old == layout
        {
            tiles.clone()
        } else {
            let rgba = layout.rasterize().ok_or(PaintError::InvalidFontTexture)?;
            let [w, h] = layout.size;
            let mut tiles = Vec::new();
            for y in (0..h).step_by(crate::atlas::PAGE_SIDE as usize) {
                for x in (0..w).step_by(crate::atlas::PAGE_SIDE as usize) {
                    let [tw, th] = [
                        (w - x).min(crate::atlas::PAGE_SIDE),
                        (h - y).min(crate::atlas::PAGE_SIDE),
                    ];
                    let mut pixels = Vec::with_capacity((tw * th * 4) as usize);
                    for row in y..y + th {
                        pixels.extend_from_slice(
                            &rgba[((row * w + x) * 4) as usize..((row * w + x + tw) * 4) as usize],
                        );
                    }
                    tiles.push(Tile {
                        image: UiTexturePage::owned([tw, th], pixels.into())
                            .map_err(|_| PaintError::InvalidFontTexture)?,
                        bounds: [x, y, x + tw, y + th],
                    });
                }
            }
            let tiles: Arc<[Tile]> = tiles.into();
            cache
                .rows
                .insert(cache_key.clone(), (layout.clone(), tiles.clone()));
            tiles
        };
        drop(cache);
        self.prepared.namespaces.insert(namespace.to_owned());
        self.prepared.rows.insert(cache_key.clone());
        for tile in tiles.iter() {
            let dest = std::array::from_fn(|edge| {
                let axis = edge % 2;
                bounds[axis]
                    + tile.bounds[edge] as f32 / layout.size[axis] as f32
                        * (bounds[axis + 2] - bounds[axis])
            });
            let start = self.nodes.len();
            self.push(UiVisual::None, dest)?;
            if self.nodes.len() == start {
                continue;
            }
            let key = format!("{cache_key}/{}-{}", tile.bounds[0], tile.bounds[1]);
            self.prepared.images.insert(key.clone(), tile.image.clone());
            self.prepared.sprites.push(PendingSprite {
                node: start,
                key,
                color,
                blend,
            });
        }
        Ok(())
    }
}

/// Uses at most one texel per whole screen pixel, retaining one texel for subpixel bounds.
/// This keeps nearest sampling from skipping thin edges during fractional minification.
fn raster_axis_size(pixels: f32) -> u32 {
    pixels.floor().max(1.) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{ImageWork, RemoteImage, RemoteSource};

    #[test]
    fn prepared_mask_edges_survive_fractional_scale_and_translation() {
        for extent in [1.1_f32, 18.3, 40.1] {
            let texels = raster_axis_size(extent);
            for phase in 0..64 {
                let origin = phase as f32 / 64.;
                let sampled: Vec<_> = (0..(origin + extent).ceil() as u32)
                    .map(|pixel| pixel as f32 + 0.5)
                    .filter(|pixel| *pixel >= origin && *pixel < origin + extent)
                    .map(|pixel| ((pixel - origin) / extent * texels as f32).floor() as u32)
                    .collect();
                assert!(sampled.contains(&0), "left edge lost at {extent}, {origin}");
                assert!(
                    sampled.contains(&(texels - 1)),
                    "right edge lost at {extent}, {origin}"
                );
            }
        }
    }

    struct Offline;
    impl RemoteSource for Offline {
        /// Prepared masks never need downloaded artwork.
        fn state(&self, _: &str) -> RemoteImage {
            RemoteImage::Failed
        }
    }

    #[test]
    fn prepared_mask_publication_preserves_transforms_tint_and_inverse_blending() {
        let set = crate::textures::TextureSet::new(0, Arc::new(Offline), ImageWork::inline());
        let mut atlas = ServerAtlas::new(&[], None, 1);
        let bounds = ui::UiRect::new(
            ui::UiPoint::new(0., 0.).unwrap(),
            ui::UiPoint::new(2., 2.).unwrap(),
        )
        .unwrap();
        let mut nodes = vec![
            UiNode::new(ui::UiNodeId::new(0), None, bounds).with_transform(ui::UiTransform {
                linear: [2., 0., 0., 2.],
                translation: [3., 4.],
            }),
        ];
        let key = "client-raster:fixture/row".to_owned();
        let prepared = PreparedRasters {
            namespaces: BTreeSet::from(["client-raster:fixture/".into()]),
            rows: Default::default(),
            images: BTreeMap::from([(
                key.clone(),
                UiTexturePage::owned([2, 2], [255, 255, 255, 64].repeat(4).into()).unwrap(),
            )]),
            sprites: vec![PendingSprite {
                node: 0,
                key: key.clone(),
                color: [255, 255, 255, 110],
                blend: UiBlendMode::Invert,
            }],
        };
        prepared.publish(&mut atlas, 0, &mut nodes, &set).unwrap();
        let draw = ui::UiTree::new(nodes).unwrap().build_draw_list().unwrap();
        assert_eq!(draw.batches[0].blend, UiBlendMode::Invert);
        assert!(
            draw.vertices
                .iter()
                .all(|v| v.color == [255, 255, 255, 110] && v.style_flags == 0)
        );
        assert_eq!(
            draw.vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::INFINITY, f32::min),
            3.
        );
        assert_eq!(
            draw.vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::NEG_INFINITY, f32::max),
            7.
        );
        let slot = atlas.placement(&key).unwrap();
        let at = (usize::from(slot.rect[1]) * crate::atlas::PAGE_SIDE as usize
            + usize::from(slot.rect[0]))
            * 4;
        assert_eq!(
            &atlas.images()[usize::from(slot.page)].pixels()[at..at + 4],
            &[255, 255, 255, 64]
        );
    }
}
