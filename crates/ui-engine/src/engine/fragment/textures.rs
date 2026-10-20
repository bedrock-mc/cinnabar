//! Retained texture requirements and sample locations for cached painted fragments.

use crate::{
    engine::{FormEngine, dependencies},
    textures::Textures,
};
use json_ui::DrawNode;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sample {
    sprite: Option<(u16, [f32; 4])>,
    native: Option<Option<usize>>,
}

/// Tracks both ordinary atlas locations and completion or relocation of original image tiles.
fn sample(textures: &Textures<'_>, path: &str) -> Sample {
    let native = textures.native_sheet(path).then(|| {
        textures
            .native_quads(path, json_ui::UvRect::full(), [0., 0., 1., 1.])
            .and_then(|mut quads| quads.next().map(|quad| quad.page))
    });
    Sample {
        sprite: textures.sprite(path),
        native,
    }
}

/// Keeps dependency keys and texture placements without rebuilding a path set every frame.
#[derive(Default)]
pub struct CachedFragmentTextures {
    nodes: Option<Arc<[DrawNode]>>,
    source: Option<(usize, u64)>,
    paths: Box<[Box<str>]>,
    keys: Vec<String>,
    samples: Vec<Sample>,
}

impl CachedFragmentTextures {
    /// Declares the fragment's paths to the enclosing screen's residency preparation.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(AsRef::as_ref)
    }

    /// Rejects stale samples if another screen changed atlas placement after preparation.
    pub fn matches(&self, textures: &Textures<'_>) -> bool {
        self.paths
            .iter()
            .zip(&self.samples)
            .all(|(path, sample)| self::sample(textures, path) == *sample)
    }
}

impl FormEngine {
    /// Keeps a fragment's textures resident and reports changed sampling locations.
    /// Stable requirements reuse their canonical keys; worker completion and eviction are checked
    /// independently of the source metadata revision.
    pub fn refresh_fragment_textures(
        &self,
        cache: &mut CachedFragmentTextures,
        nodes: &Arc<[DrawNode]>,
    ) -> bool {
        let changed_nodes = !cache
            .nodes
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, nodes));
        if changed_nodes {
            cache.paths = dependencies::retained_paths(nodes);
            cache.nodes = Some(Arc::clone(nodes));
        }
        if cache.paths.is_empty() {
            cache.samples.clear();
            cache.keys.clear();
            return false;
        }
        let source = (self as *const _ as usize, self.textures.metadata_revision());
        let mut atlas = self.textures.lock();
        if changed_nodes || cache.source != Some(source) {
            let textures = Textures {
                assets: &self.assets,
                set: &self.textures,
                atlas: &atlas,
                images: None,
            };
            cache.keys = textures
                .atlas_keys(cache.paths())
                .into_iter()
                .map(|key| key.into_owned())
                .collect();
            cache.source = Some(source);
        }
        atlas.require(cache.keys.iter().map(String::as_str));
        let textures = Textures {
            assets: &self.assets,
            set: &self.textures,
            atlas: &atlas,
            images: None,
        };
        let mut changed = cache.samples.len() != cache.paths.len();
        cache.samples.resize(cache.paths.len(), Sample::default());
        for (path, sample) in cache.paths.iter().zip(&mut cache.samples) {
            let current = self::sample(&textures, path);
            changed |= *sample != current;
            *sample = current;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        atlas::ServerAtlas,
        engine::screen::ScreenRegistry,
        sources::{ImageWork, RemoteImage, RemoteSource},
    };
    use std::{
        io::Cursor,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    struct NoImages;
    impl RemoteSource for NoImages {
        /// This fixture uses only original in-memory pack images.
        fn state(&self, _: &str) -> RemoteImage {
            RemoteImage::Failed
        }
    }

    /// Encodes original pixels that each occupy one bounded atlas page.
    fn png(size: [u32; 2], color: [u8; 4]) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(size[0], size[1], image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn hud_card_preview_textures_follow_worker_completion_and_eviction_without_metadata_changes() {
        static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
        let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
        let clock = Arc::new(AtomicU64::new(0));
        let work = ImageWork {
            workers: true,
            clock: Some(Arc::new(move || {
                Duration::from_secs(clock.fetch_add(1, Ordering::Relaxed))
            })),
        };
        let mut engine = FormEngine::new(
            Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
            json_ui::Catalog::default(),
            0,
            REGISTRY.get_or_init(|| ScreenRegistry::new(json_ui::Context::default())),
            Arc::new(NoImages),
            work.clone(),
        );
        engine.set_server_atlas(
            ServerAtlas::new(
                &[
                    ("textures/a.png".into(), png([200; 2], [255, 0, 0, 255])),
                    ("textures/b.png".into(), png([200; 2], [0, 0, 255, 255])),
                    (
                        "textures/native.png".into(),
                        png([render_model::MAX_UI_TEXTURE_SIDE + 1, 4], [0, 255, 0, 255]),
                    ),
                ],
                None,
                1,
            ),
            crate::pages::SERVER_UI_PAGE as u16,
        );
        let nodes: Arc<[DrawNode]> = Arc::from([DrawNode {
            name: "image".into(),
            key: "image".into(),
            dest: json_ui::RectOut {
                x: 0.,
                y: 0.,
                w: 20.,
                h: 20.,
            },
            clip: json_ui::RectOut {
                x: 0.,
                y: 0.,
                w: 100.,
                h: 100.,
            },
            layer: 0,
            alpha: 1.,
            anim: None,
            gates: vec![],
            draw: json_ui::Draw::Sprite {
                texture: "textures/a".into(),
                uv: json_ui::UvRect::full(),
                repeat: None,
                color: [255; 4],
                filter: Default::default(),
            },
        }]);
        let mut cache = CachedFragmentTextures::default();
        engine.refresh_fragment_textures(&mut cache, &nodes);
        assert_eq!(cache.samples, [Sample::default()]);
        let metadata = engine.textures.metadata_revision();
        test_time::eventually("HUD preview worker texture becomes resident", || {
            engine.refresh_fragment_textures(&mut cache, &nodes);
            cache.samples[0].sprite.is_some()
        });
        assert_eq!(metadata, engine.textures.metadata_revision());
        let (_, allocation) =
            test_alloc::measure(|| engine.refresh_fragment_textures(&mut cache, &nodes));
        assert_eq!(allocation.calls, 0);
        test_time::eventually("other artwork evicts the cached preview texture", || {
            let mut atlas = engine.textures.lock();
            atlas.require(["textures/b"]);
            atlas.placement("textures/b").is_some()
        });
        {
            let atlas = engine.textures.lock();
            let textures = Textures {
                assets: &engine.assets,
                set: &engine.textures,
                atlas: &atlas,
                images: None,
            };
            assert!(!cache.matches(&textures));
        }
        engine.refresh_fragment_textures(&mut cache, &nodes);
        {
            let atlas = engine.textures.lock();
            let textures = Textures {
                assets: &engine.assets,
                set: &engine.textures,
                atlas: &atlas,
                images: None,
            };
            assert!(cache.matches(&textures));
            assert!(cache.samples[0].sprite.is_some());
        }
        let mut native_node = nodes[0].clone();
        if let json_ui::Draw::Sprite { texture, .. } = &mut native_node.draw {
            *texture = "textures/native".into();
        }
        let native_nodes = Arc::from([native_node]);
        let mut native = CachedFragmentTextures::default();
        engine.refresh_fragment_textures(&mut native, &native_nodes);
        assert_eq!(native.samples[0].native, Some(None));
        test_time::eventually("native preview image tiles become resident", || {
            engine.refresh_fragment_textures(&mut native, &native_nodes);
            native.samples[0].native.flatten().is_some()
        });
        assert_eq!(metadata, engine.textures.metadata_revision());
    }
}
