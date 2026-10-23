use std::{collections::HashMap, io::Cursor, sync::OnceLock};

use json_ui::{Catalog, Context, FormRender, ResolvedControl};
use ui::{DpiScale, IconRef, SafeArea, TextLayoutCache};

use super::*;
use crate::{
    atlas::ServerAtlas,
    engine::{
        FormEngine,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

struct NoDownloads;

impl RemoteSource for NoDownloads {
    /// These local source fixtures never download an image.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// An engine with synthetic carrier/font fixtures and inline image work, requiring no files.
fn engine() -> FormEngine {
    static REGISTRY: OnceLock<ScreenRegistry> = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop()));
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        registry,
        Arc::new(NoDownloads),
        ImageWork::inline(),
    )
}

/// Encodes a source image small enough to stay on the ordinary atlas path.
fn png(side: u32, value: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(side, side, image::Rgba([value, 0, 0, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Publishes nodes as a retained render while leaving unrelated binding and geometry empty.
fn render(nodes: Vec<DrawNode>) -> FormRender {
    FormRender {
        bound: ResolvedControl {
            name: "screen".into(),
            control_type: None,
            base: None,
            unresolved_base: None,
            properties: Default::default(),
            children: Vec::new(),
            factory: None,
        },
        nodes: nodes.into(),
        hits: Arc::from([]),
        report: Default::default(),
        cancel_target: None,
        root_panel: None,
    }
}

/// Exercises the actual prepare path with the same retained layout and current image map.
fn prepare(engine: &FormEngine, render: &FormRender, images: Option<&HashMap<String, IconRef>>) {
    engine.textures.lock().begin_frame();
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(16, 16 * 1024);
    let mut painted = Vec::new();
    let mut next = 1;
    engine
        .prepare(
            ScreenArt {
                images,
                ..Default::default()
            },
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics: TextMetrics::for_viewport([640, 480], DpiScale::new(1.0).unwrap(), None),
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [640.0, 480.0],
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut painted,
                next: &mut next,
                renderers: &mut [],
            },
            |_, _| Some(render),
        )
        .expect("retained fixture prepares");
}

#[test]
fn cache_hits_still_require_evicted_images_and_resolve_overrides_and_replaced_sources() {
    let mut engine = engine();
    let files = vec![
        ("textures/a.png".into(), png(200, 11)),
        ("textures/b.png".into(), png(200, 22)),
    ];
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    let render = render(vec![sprite("/image", "textures/a.png")]);
    prepare(&engine, &render, None);
    assert!(engine.textures.lock().placement("textures/a").is_some());
    engine.textures.lock().require(["textures/b"]);
    assert!(engine.textures.lock().placement("textures/a").is_none());
    prepare(&engine, &render, None);
    assert!(
        engine.textures.lock().placement("textures/a").is_some(),
        "a cache hit requires an evicted source again"
    );

    engine.textures.lock().require(["textures/b"]);
    let images = HashMap::from([(
        "textures/a.png".into(),
        IconRef {
            page: 99,
            uv: [0, 0, 16, 16],
            glint: false,
        },
    )]);
    prepare(&engine, &render, Some(&images));
    assert!(
        engine.textures.lock().placement("textures/a").is_none(),
        "current artwork overrides bypass the atlas"
    );
    prepare(&engine, &render, None);
    assert!(engine.textures.lock().placement("textures/a").is_some());
    assert_eq!(engine.dependencies.lock().unwrap().discoveries, 1);

    let replacement = [("textures/a.png".into(), png(32, 77))];
    engine.set_server_atlas(ServerAtlas::new(&replacement, None, 1), 4);
    prepare(&engine, &render, None);
    let atlas = engine.textures.lock();
    let placed = atlas.placement("textures/a").unwrap();
    assert_eq!(placed.rect[2..], [32, 32]);
    let [x, y, _, _] = placed.rect;
    let page = &atlas.images()[usize::from(placed.page)];
    let offset = ((u32::from(y) * page.dimensions()[0] + u32::from(x)) * 4) as usize;
    assert_eq!(&page.pixels()[offset..offset + 4], &[77, 0, 0, 255]);
    drop(atlas);
    assert_eq!(
        engine.dependencies.lock().unwrap().discoveries,
        1,
        "only logical paths were retained"
    );

    engine.install_pack_catalog(Arc::new(Catalog::default()), None);
    assert!(engine.dependencies.lock().unwrap().entries.is_empty());
    prepare(&engine, &render, None);
    assert_eq!(engine.dependencies.lock().unwrap().discoveries, 1);
}
