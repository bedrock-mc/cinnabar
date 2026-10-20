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

type CachedRow = (MaskLayout, [u32; 4], Arc<[Tile]>);

#[derive(Default)]
pub(crate) struct MaskCache {
    rows: BTreeMap<String, CachedRow>,
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
    sources: BTreeSet<(String, [u32; 2])>,
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
        for (key, size) in self.sources {
            atlas.prepare_outline(&key, size);
        }
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
    pub fn outline_mask(&self, path: &str, size: [u32; 2]) -> Option<Arc<SpriteMask>> {
        self.admits(path)
            .then(|| self.atlas.original_outline(&self.canonical(path), size))
            .flatten()
    }
}

impl Canvas<'_> {
    /// Requests draw-sized source coverage after paint; until it is ready the overlay is skipped.
    pub fn outline_mask(&mut self, path: &str, size: [u32; 2]) -> Option<Arc<SpriteMask>> {
        if !self.textures.admits(path) {
            return None;
        }
        self.prepared
            .sources
            .insert((self.textures.canonical(path).into_owned(), size));
        self.textures.outline_mask(path, size)
    }

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
        self.prepared.namespaces.insert(namespace.to_owned());
        let Some(clip) = self.clip() else {
            return Ok(());
        };
        let viewport = [
            clip[0].max(self.screen[0]),
            clip[1].max(self.screen[1]),
            clip[2].min(self.screen[2]),
            clip[3].min(self.screen[3]),
        ];
        let Some(raster) = visible_mask_rect(layout.size, bounds, viewport, self.residual) else {
            return Ok(());
        };
        let [left, top, right, bottom] = raster;
        let [w, h] = [(right - left) as usize, (bottom - top) as usize];
        let page = crate::atlas::PAGE_SIDE as usize;
        let count = w.div_ceil(page).saturating_mul(h.div_ceil(page));
        let bytes = w.saturating_mul(h).saturating_mul(4);
        let (queued_count, queued_bytes) = self
            .prepared
            .images
            .iter()
            .filter(|(key, _)| key.starts_with(namespace))
            .fold((0_usize, 0_usize), |(count, bytes), (_, image)| {
                (count + 1, bytes + image.pixels().len())
            });
        if !self.textures.atlas.admits_client_rasters(
            namespace,
            queued_count.saturating_add(count),
            bytes.saturating_add(queued_bytes),
        ) {
            return Ok(());
        }
        let cache_key = format!("{namespace}{key}");
        let mut cache = self
            .textures
            .set
            .masks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let tiles = if let Some((old, old_raster, tiles)) = cache.rows.get(&cache_key)
            && *old == layout
            && *old_raster == raster
        {
            tiles.clone()
        } else {
            let mut tiles = Vec::with_capacity(count);
            for y in (top..bottom).step_by(page) {
                for x in (left..right).step_by(page) {
                    let [tw, th] = [
                        (right - x).min(crate::atlas::PAGE_SIDE),
                        (bottom - y).min(crate::atlas::PAGE_SIDE),
                    ];
                    let pixels = layout
                        .rasterize_rect([x, y, x + tw, y + th])
                        .ok_or(PaintError::InvalidFontTexture)?;
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
                .insert(cache_key.clone(), (layout.clone(), raster, tiles.clone()));
            tiles
        };
        drop(cache);
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

/// Maps the viewport back into layout texels; rotation uses conservative corner bounds.
fn visible_mask_rect(
    size: [u32; 2],
    bounds: [f32; 4],
    viewport: [f32; 4],
    transform: Option<json_ui::transform::Affine>,
) -> Option<[u32; 4]> {
    if viewport[2] <= viewport[0]
        || viewport[3] <= viewport[1]
        || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
    {
        return None;
    }
    let inverse = transform
        .unwrap_or(json_ui::transform::Affine::IDENTITY)
        .inverse()?;
    let visible = inverse.bounds(json_ui::Rect {
        x: f64::from(viewport[0]),
        y: f64::from(viewport[1]),
        w: f64::from(viewport[2] - viewport[0]),
        h: f64::from(viewport[3] - viewport[1]),
    });
    let edges = [
        visible.x,
        visible.y,
        visible.x + visible.w,
        visible.y + visible.h,
    ];
    let raster: [u32; 4] = std::array::from_fn(|edge| {
        let axis = edge % 2;
        let value = (edges[edge] - f64::from(bounds[axis]))
            / f64::from(bounds[axis + 2] - bounds[axis])
            * f64::from(size[axis]);
        (if edge < 2 {
            value.floor()
        } else {
            value.ceil()
        })
        .clamp(0., f64::from(size[axis])) as u32
    });
    (raster[2] > raster[0] && raster[3] > raster[1]).then_some(raster)
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
    fn scaled_mask_allocates_only_visible_pixels_and_checks_admission_first() {
        use render_api::sprite_mask::MaskPatch;
        let assets = assets::RuntimeUiAssets::decode(
            &assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap(),
        )
        .unwrap();
        let set = crate::textures::TextureSet::new(0, Arc::new(Offline), ImageWork::inline());
        let atlas = ServerAtlas::new(&[], None, 1);
        let font = crate::test_support::fixture_font();
        let animator = std::sync::Mutex::new(json_ui::Animator::default());
        let mut canvas = Canvas {
            prepared: Default::default(),
            edit: None,
            textures: Textures {
                assets: &assets,
                set: &set,
                atlas: &atlas,
                images: None,
            },
            solid_page: 0,
            art: Default::default(),
            now: 0.,
            screen: [0., 0., 64., 48.],
            layouts: &mut ui::TextLayoutCache::new(4, 1 << 20),
            font: &font,
            metrics: crate::text_metrics::TextMetrics::for_viewport(
                [64, 48],
                ui::DpiScale::new(1.).unwrap(),
                Some(1),
            ),
            px: 1.,
            translate: &|_| None,
            nodes: &mut Vec::new(),
            next: &mut 0,
            clip: None,
            animator: animator.lock().unwrap(),
            length_scale: 1.,
            residual: Some(json_ui::transform::Affine {
                linear: [64., 0., 0., 64.],
                translation: [-128., -64.],
            }),
            filter: None,
        };
        canvas.group(canvas.screen).unwrap();
        let bounds = [0., 0., 81., 11.];
        let mut layout = MaskLayout {
            source: Arc::new(SpriteMask::outline([9; 2], [9; 2], |_, _| 255)),
            extent: [81., 11.],
            size: canvas.raster_size(bounds),
            patches: vec![MaskPatch {
                bounds,
                uv: [0., 0., 1., 1.],
                opacity: 255,
            }],
        };
        assert_eq!(layout.size, [5184, 704]);
        let (_, allocations) = test_alloc::measure(|| {
            canvas
                .mask(
                    "client-raster:test/",
                    "row",
                    layout.clone(),
                    bounds,
                    [255; 4],
                    UiBlendMode::Alpha,
                )
                .unwrap()
        });
        assert!(
            allocations.bytes < 64 * 48 * 16,
            "visible row requested {allocations:?}"
        );
        let image = canvas.prepared.images.values().next().unwrap();
        assert_eq!(image.dimensions(), [64, 48]);
        assert!(
            image
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255; 4])
        );
        assert_eq!(
            canvas.nodes[1].bounds().min().x(),
            2.,
            "cropping preserves row coordinates"
        );
        canvas.prepared = Default::default();
        canvas.screen = [0., 0., 4096., 2304.];
        canvas.group(canvas.screen).unwrap();
        canvas.residual = None;
        layout.size = [4096, 2304];
        let (_, rejected) = test_alloc::measure(|| {
            canvas
                .mask(
                    "client-raster:test/",
                    "huge",
                    layout,
                    canvas.screen,
                    [255; 4],
                    UiBlendMode::Alpha,
                )
                .unwrap()
        });
        assert!(rejected.bytes < 1024, "rejected row requested {rejected:?}");
        assert!(canvas.prepared.images.is_empty());
    }

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

    #[test]
    fn checkerboard_source_keeps_row_paint_allocations_proportional_to_output() {
        use render_api::sprite_mask::MaskPatch;
        let source = Arc::new(SpriteMask::outline([1024; 2], [18; 2], |x, y| {
            if (x + y).is_multiple_of(2) { 255 } else { 0 }
        }));
        let mut layout = MaskLayout {
            source,
            extent: [81., 11.],
            size: [162, 22],
            patches: (0..10)
                .map(|icon| MaskPatch {
                    bounds: [icon as f32 * 8., 1., icon as f32 * 8. + 9., 10.],
                    uv: [0., 0., 1., 1.],
                    opacity: 255,
                })
                .collect(),
        };
        for change in 0..3 {
            layout.patches[0].opacity = 255 - change * 50;
            layout.patches[1].bounds[1] = 1. + f32::from(change % 2);
            layout.patches[1].bounds[3] = layout.patches[1].bounds[1] + 9.;
            let (pixels, allocations) = test_alloc::measure(|| layout.rasterize().unwrap());
            assert_eq!(pixels.len(), 162 * 22 * 4);
            assert!(
                allocations.calls <= 8,
                "row paint requested {allocations:?}"
            );
            assert!(
                allocations.bytes <= 162 * 22 * 32,
                "row paint requested {allocations:?}"
            );
            assert!((127..=128).contains(&pixels[(8 * 162 + 80) * 4 + 3]));
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
            sources: BTreeSet::new(),
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
