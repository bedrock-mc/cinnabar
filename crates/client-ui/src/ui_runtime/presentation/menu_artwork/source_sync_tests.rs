//! Artwork synchronization through the painted-frame publication boundary.

use super::*;
use crate::ui_runtime::presentation::dynamic_textures;
use ui_engine::{
    atlas::ServerAtlas,
    sources::{ImageWork, RemoteImage, RemoteSource},
    textures::TextureSet,
};

/// Encodes an original oversized source with deterministic pixels.
fn png(color: u8) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([color, 2, 3, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// A download whose completion the test publishes without wall-clock waits.
struct Download(std::sync::Mutex<RemoteImage>);

impl RemoteSource for Download {
    /// Returns the current asynchronous delivery state.
    fn state(&self, _: &str) -> RemoteImage {
        self.0.lock().unwrap().clone()
    }
}

/// Uses inline image work so source publication depends only on explicit test actions.
fn presentation() -> UiPresentationRuntime {
    let mut runtime = crate::test_support::mini_engine_presentation();
    let textures = &mut runtime.engine.as_mut().unwrap().textures;
    let (first, server) = (textures.first_page, textures.server_page);
    *textures = TextureSet::new(
        first,
        Arc::new(Download(std::sync::Mutex::new(RemoteImage::Failed))),
        ImageWork::inline(),
    );
    textures.set_atlas(ServerAtlas::new(&[], None, 1), server);
    runtime.finish_menu_artwork();
    runtime.sync_server_ui_pages();
    runtime
}

/// Replaces the atlas and draws its named sources before the real publication boundary.
fn install(runtime: &mut UiPresentationRuntime, files: &[(&str, Vec<u8>)]) {
    let owned: Vec<_> = files
        .iter()
        .map(|(key, bytes)| (format!("{key}.png"), bytes.clone()))
        .collect();
    let textures = &mut runtime.engine.as_mut().unwrap().textures;
    textures.set_atlas(ServerAtlas::new(&owned, None, 1), textures.server_page);
    textures
        .atlas_mut()
        .require(files.iter().map(|entry| entry.0));
}

#[test]
fn unchanged_painted_publications_reuse_metadata_allocate_nothing_and_request_nothing() {
    let mut runtime = presentation();
    install(&mut runtime, &[("z", png(1)), ("a", png(2))]);
    let bytes: Arc<[u8]> = png(3).into();
    runtime.menu_artwork_set.paths = (0..128)
        .map(|i| (format!("absent-fixture-{i}"), 128))
        .collect();
    runtime.menu_artwork_set.images = vec![("fixture-image".into(), bytes.clone())];
    runtime.menu_artwork_set.skins = vec![SkinArtwork {
        key: "skin".into(),
        skin: render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: None,
        },
    }];
    runtime.menu_artwork_set.capes = vec![CapeArtwork {
        key: "cape".into(),
        cape: render_api::CapeImage {
            width: 64,
            height: 32,
            rgba8: vec![255; 64 * 32 * 4].into(),
        },
        thumbnail: false,
    }];
    runtime.sync_server_ui_pages();
    runtime.finish_menu_artwork();
    let source = runtime.oversized_ui_textures();
    let requested = runtime.menu_artwork_loader.requested;
    let paths = runtime.menu_artwork_set.paths.as_ptr();
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..100 {
            runtime.sync_server_ui_pages();
            assert!(runtime.oversized_ui_textures().shares_storage(&source));
        }
    });
    assert_eq!(allocations.calls, 0);
    assert_eq!(allocations.bytes, 0);
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
    assert!(Arc::ptr_eq(&runtime.menu_artwork_set.images[0].1, &bytes));
    assert_eq!(runtime.menu_artwork_set.paths.as_ptr(), paths);
    assert_eq!(
        source
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
}

#[test]
fn replaced_removed_and_equal_byte_sources_publish_without_generation_collisions() {
    let mut runtime = presentation();
    let bytes = png(4);
    install(&mut runtime, &[("art", bytes.clone())]);
    let local_revision = runtime
        .engine
        .as_ref()
        .unwrap()
        .textures
        .lock()
        .metadata_revision();
    runtime.sync_server_ui_pages();
    let original = runtime.menu_artwork_set.oversized.clone();
    let mut requested = runtime.menu_artwork_loader.requested;
    for replacement in [bytes.clone(), png(9)] {
        install(&mut runtime, &[("art", replacement)]);
        assert_eq!(
            runtime
                .engine
                .as_ref()
                .unwrap()
                .textures
                .lock()
                .metadata_revision(),
            local_revision
        );
        runtime.sync_server_ui_pages();
        requested += 1;
        assert_eq!(runtime.menu_artwork_loader.requested, requested);
        assert!(!Arc::ptr_eq(
            &runtime.menu_artwork_set.oversized[0].1,
            &original[0].1
        ));
    }
    assert_eq!(original[0].1.as_ref(), bytes.as_slice());
    install(&mut runtime, &[("new", png(7))]);
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_set.oversized[0].0, "new");
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 1);
    install(&mut runtime, &[]);
    runtime.sync_server_ui_pages();
    assert!(runtime.menu_artwork_set.oversized.is_empty());
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 2);
}

#[test]
fn completed_download_requests_art_and_evicted_fitted_pages_keep_the_snapshot() {
    let mut runtime = presentation();
    let download = Arc::new(Download(std::sync::Mutex::new(RemoteImage::Loading)));
    let textures = &mut runtime.engine.as_mut().unwrap().textures;
    textures.remote = download.clone();
    textures.set_atlas(
        ServerAtlas::new(&[("other.png".into(), png(2))], None, 1),
        textures.server_page,
    );
    let key = "https://fixture.invalid/art.png";
    textures.atlas_mut().require([key]);
    runtime.sync_server_ui_pages();
    let requested = runtime.menu_artwork_loader.requested;
    *download.0.lock().unwrap() = RemoteImage::Ready(render_api::BodyGeneration(1), png(1).into());
    runtime
        .engine
        .as_mut()
        .unwrap()
        .textures
        .atlas_mut()
        .require([key]);
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 1);
    runtime
        .engine
        .as_mut()
        .unwrap()
        .textures
        .atlas_mut()
        .require(["other"]);
    runtime.sync_server_ui_pages();
    let retained = runtime.menu_artwork_set.oversized.clone();
    let requested = runtime.menu_artwork_loader.requested;
    assert!(
        runtime
            .engine
            .as_ref()
            .unwrap()
            .textures
            .lock()
            .placement(key)
            .is_none()
    );
    runtime
        .engine
        .as_mut()
        .unwrap()
        .textures
        .atlas_mut()
        .require([key]);
    runtime.sync_server_ui_pages();
    assert!(runtime.menu_artwork_set.oversized.shares_storage(&retained));
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
    runtime.finish_menu_artwork();
    let art = runtime.menu_artwork.refs[&format!("{SERVER_ART_PREFIX}{key}")];
    let engine = runtime.engine.as_ref().unwrap();
    let lock = engine.textures.lock();
    let textures = ui_engine::textures::Textures {
        assets: engine.assets(),
        set: &engine.textures,
        atlas: &lock,
        images: None,
    };
    assert_eq!(textures.sprite(key).unwrap().0, art.page);
    assert_eq!(
        art.uv[2] - art.uv[0],
        512,
        "packing refreshes the full-resolution mapping"
    );
}

#[test]
fn absent_engine_and_session_reset_do_not_reuse_a_stale_snapshot() {
    let mut runtime = presentation();
    install(&mut runtime, &[("art", png(1))]);
    runtime.sync_server_ui_pages();
    let engine = runtime.engine.take();
    let requested = runtime.menu_artwork_loader.requested;
    runtime.sync_server_ui_pages();
    assert!(runtime.menu_artwork_set.oversized.is_empty());
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 1);
    let (_, allocations) = test_alloc::measure(|| runtime.sync_server_ui_pages());
    assert_eq!(allocations.calls, 0);
    runtime.engine = engine;
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 2);
    dynamic_textures::observe_session(&mut runtime, 1);
    dynamic_textures::observe_session(&mut runtime, 2);
    assert!(runtime.menu_artwork_set.oversized.is_empty());
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 3);
}

#[test]
fn unchanged_gallery_polls_and_installs_worker_results_and_resets_its_request() {
    use launcher::menu::{MenuScreen, MenuView};
    let mut runtime = presentation();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::DressingRoom;
    runtime.sync_menu_artwork_view(&view);
    runtime.finish_menu_artwork();
    assert!(runtime.menu_artwork_loader.gallery.is_some());
    let requested = runtime.menu_artwork_loader.requested;
    let (done, results) = crossbeam_channel::unbounded();
    runtime.menu_artwork_loader.results = results;
    let side = render_model::UI_ART_PAGE_SIDE;
    done.send(Packed {
        id: requested,
        complete: true,
        pages: vec![
            render_model::UiTexturePage::owned(
                [side; 2],
                vec![71; (side * side * 4) as usize].into(),
            )
            .unwrap(),
        ],
        refs: HashMap::from([(
            "completed-gallery".into(),
            IconRef {
                page: 0,
                uv: [0, 0, 2, 2],
                glint: false,
            },
        )]),
    })
    .unwrap();
    runtime.sync_menu_artwork_view(&view);
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
    let icon = runtime.menu_artwork_icon("completed-gallery").unwrap();
    assert_eq!(
        runtime.textures.pages()[usize::from(icon.page)].pixels()[..4],
        [71; 4]
    );
    assert!(runtime.menu_artwork_loader.pending().is_none());
    dynamic_textures::observe_session(&mut runtime, 1);
    dynamic_textures::observe_session(&mut runtime, 2);
    assert!(runtime.menu_artwork_loader.gallery.is_none());
    runtime.sync_menu_artwork_view(&view);
    assert!(runtime.menu_artwork_loader.gallery.is_some());
}

#[test]
fn same_sources_restore_full_resolution_after_an_atlas_reinstallation() {
    let mut runtime = presentation();
    install(&mut runtime, &[("art", png(1))]);
    runtime.sync_server_ui_pages();
    runtime.finish_menu_artwork();
    let requested = runtime.menu_artwork_loader.requested;
    let engine = runtime.engine.as_mut().unwrap();
    let atlas = std::mem::take(engine.textures.atlas_mut());
    engine
        .textures
        .set_atlas(atlas, engine.textures.server_page);
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
    let engine = runtime.engine.as_ref().unwrap();
    let lock = engine.textures.lock();
    let textures = ui_engine::textures::Textures {
        assets: engine.assets(),
        set: &engine.textures,
        atlas: &lock,
        images: None,
    };
    let sprite = textures.sprite("art").unwrap();
    assert_eq!(sprite.1[2..], [512.0, 512.0]);
}

/// An installed source whose encoded length exercises background file completion.
struct LocalImage(Vec<u8>);

impl ui_engine::sources::LocalPack for LocalImage {
    /// Reports just the named fixture image.
    fn file_size(&self, relative: &str) -> Option<u64> {
        (relative == "textures/async.png").then_some(self.0.len() as u64)
    }

    /// Returns bounded fixture bytes to the source-read worker.
    fn read(&self, relative: &str, limit: u64) -> Option<Vec<u8>> {
        (relative == "textures/async.png" && self.0.len() as u64 <= limit).then(|| self.0.clone())
    }
}

#[test]
fn asynchronous_file_completion_still_polls_and_requests_new_artwork() {
    let mut runtime = presentation();
    let mut bytes = png(19);
    bytes.resize(resource_pack::MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    let textures = &mut runtime.engine.as_mut().unwrap().textures;
    let (first, server) = (textures.first_page, textures.server_page);
    *textures = TextureSet::new(
        first,
        Arc::new(Download(std::sync::Mutex::new(RemoteImage::Failed))),
        ImageWork::default(),
    );
    textures.set_atlas(ServerAtlas::new(&[], None, 1), server);
    textures.set_fallbacks(HashMap::new(), Arc::new(LocalImage(bytes)));
    assert!(textures.lock().fallback_size("textures/async").is_none());
    runtime.sync_server_ui_pages();
    let requested = runtime.menu_artwork_loader.requested;
    test_time::eventually(
        "installed image completion reaches the presentation",
        || {
            runtime.poll_server_ui_images();
            runtime
                .engine
                .as_ref()
                .unwrap()
                .textures
                .lock()
                .fallback_size("textures/async")
                .is_some()
        },
    );
    runtime
        .engine
        .as_mut()
        .unwrap()
        .textures
        .atlas_mut()
        .require(["textures/async"]);
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested + 1);
    assert_eq!(runtime.menu_artwork_set.oversized[0].0, "textures/async");
    runtime.finish_menu_artwork();
    assert!(
        runtime
            .menu_artwork_icon(&format!("{SERVER_ART_PREFIX}textures/async"))
            .is_some()
    );
}

#[test]
fn simultaneous_mod_modal_and_server_pages_all_publish_without_an_artwork_change() {
    use mod_ui::{experience_modal::ExperienceModal, mod_screens::ModScreensInput};
    use server_experience::screen;
    use ui_engine::{engine::screen::PaintFrame, frame::EngineFrame, text_metrics::TextMetrics};
    let mut runtime = presentation();
    let files = Arc::new(screen::Files {
        namespace: "fixture".into(),
        templates: [("ui/panel.json".into(), br#"{"namespace":"fixture","panel":{"type":"image","size":[16,16],"texture":"textures/fixture"}}"#.to_vec())].into(),
        textures: vec![("textures/fixture.png".into(), png(31))],
    });
    let mut modal = screen::Modal::default();
    modal.open(Some("ui/panel.json".into()));
    runtime.set_experience_modal(Some(ExperienceModal {
        bundle: "fixture",
        files: &files,
        modal: &modal,
    }));
    let template = "ui/panel.json".to_owned();
    runtime.set_mod_screens(Some(ModScreensInput {
        id: "fixture",
        files: &files,
        overlay: None,
        view: Some(&template),
        focus: None,
        data: &modal,
    }));
    let container = EngineFrame {
        hits: Arc::from([]),
        report: Default::default(),
        cancel_target: None,
        origin: [0.0; 2],
        scale: 1.0,
        panel: Some([20.0, 20.0, 32.0, 32.0]),
        edit_texts: Vec::new(),
        top: Vec::new(),
    };
    let mut nodes = Vec::new();
    let mut next = 1;
    let mut paint = PaintFrame {
        layouts: &mut runtime.layouts,
        font: &runtime.font,
        metrics: TextMetrics::for_viewport([640, 480], ui::DpiScale::new(1.0).unwrap(), None),
        solid_page: runtime.solid_texture_page,
        safe_area: runtime.safe_area,
        content: [640.0, 480.0],
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    runtime.screens.mod_ui.paint_experience_modal(
        runtime.engine.as_deref(),
        &mut paint,
        true,
        &[],
        |_| Default::default(),
    );
    runtime.screens.mod_ui.paint_mod_screens(
        runtime.engine.as_deref(),
        Some(&container),
        &mut paint,
        0,
        &[],
        || Some("fixture"),
    );
    install(
        &mut runtime,
        &[("small", {
            let mut bytes = Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(4, 4, image::Rgba([47; 4]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        })],
    );
    let requested = runtime.menu_artwork_loader.requested;
    runtime.sync_server_ui_pages();
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
    for pages in [
        runtime.screens.mod_ui.experience_modal_pages(),
        runtime.screens.mod_ui.mod_screen_pages(),
        runtime.server_ui_pages(),
    ] {
        assert!(
            !pages.is_empty(),
            "every source published its changed pages"
        );
        let published = &pages[0];
        assert!(
            runtime
                .textures
                .pages()
                .iter()
                .any(|page| page.identity() == published.identity())
        );
    }
}

#[test]
fn equivalent_source_metadata_is_compared_once_then_retained_without_requests() {
    let mut runtime = presentation();
    install(&mut runtime, &[("art", png(3))]);
    runtime.sync_server_ui_pages();
    let source = runtime.oversized_ui_textures();
    runtime.menu_artwork_set.oversized = source.iter().cloned().collect();
    assert!(!source.shares_storage(&runtime.menu_artwork_set.oversized));
    let requested = runtime.menu_artwork_loader.requested;
    runtime.sync_server_ui_pages();
    assert!(source.shares_storage(&runtime.menu_artwork_set.oversized));
    let (_, allocations) = test_alloc::measure(|| runtime.sync_server_ui_pages());
    assert_eq!(allocations.calls, 0);
    assert_eq!(runtime.menu_artwork_loader.requested, requested);
}
