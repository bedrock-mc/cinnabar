//! Offline reload evidence: no transport or server is constructed by these tests.

use super::{
    pack_reload::{PackInputs, PackReload, reload_resource_packs},
    resource_packs,
};
use crate::runtime::world::ClientWorld;
use bevy::prelude::{App, Update};
use client_ui::ui_runtime::UiRuntime;
use resource_pack::ValidatedPackStack;
use std::{
    io::{Cursor, Write},
    sync::Arc,
    time::{Duration, Instant},
};

/// Builds original test data into the same archive admission path used at runtime.
pub(super) fn stack(files: &[(&str, &[u8])]) -> Arc<ValidatedPackStack> {
    let id = "00000000-0000-0000-0000-00000000000a";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
        protocol::ResourcePackArchive::unencrypted(
            id.parse().unwrap(),
            "1.0.0".into(),
            String::new(),
            zip.finish().unwrap().into_inner(),
        ),
    ]))
}

/// A genuine in-memory world stream survives the same worker/publication system used in the app.
fn app() -> App {
    app_with_assets(Arc::new(assets::RuntimeAssets::diagnostic()))
}

/// Uses an explicitly supplied carrier for offline world rendering.
pub(super) fn app_with_assets(assets: Arc<assets::RuntimeAssets>) -> App {
    let mut world = ClientWorld::new(assets.clone());
    let bootstrap = protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 7,
        local_player_unique_id: 7,
        player_position: [0.0, 65.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: assets
            .air_network_id(assets::NetworkIdMode::Sequential)
            .unwrap_or(0),
        block_network_ids_are_hashes: false,
    };
    world.stream = Some(chunk_pipeline::WorldStream::new_with_assets(
        bootstrap,
        assets.clone(),
        bootstrap.player_position,
        None,
    ));
    let mut app = App::new();
    app.insert_resource(world)
        .insert_resource(UiRuntime::new(0))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(0))
        .insert_resource(render::ChunkTextureAssets::with_revision(assets, 0))
        .init_resource::<PackReload>()
        .add_systems(Update, reload_resource_packs);
    app
}

/// Drives frame updates until publication, measuring CPU update cost independently of worker time.
pub(super) fn settle(app: &mut App, revision: u64) -> (Duration, Duration) {
    let start = Instant::now();
    let mut peak = Duration::ZERO;
    test_time::eventually_within(Duration::from_secs(120), "the reload worker", || {
        if app.world().resource::<PackReload>().completed_revision() >= revision {
            return true;
        }
        let frame = Instant::now();
        app.update();
        peak = peak.max(frame.elapsed());
        false
    });
    (start.elapsed(), peak)
}

#[test]
fn optional_reload_reuses_unchanged_subscribers_and_removes_absent_ui() {
    let definition = br#"{"namespace":"reload_fixture","panel":{"type":"panel","size":[23,17]}}"#;
    let first = resource_packs::prepare_validated_application(
        stack(&[("ui/reload.json", definition)]),
        Arc::new(PackInputs::default()),
    );
    let next = resource_packs::prepare_changed_application(
        &resource_packs::reuse::CompiledStacks::new(),
        stack(&[
            ("ui/reload.json", definition),
            ("texts/en_US.lang", b"test.reload=changed"),
        ]),
        Arc::new(PackInputs::default()),
        Some(&first),
    );
    assert!(Arc::ptr_eq(
        first.server_ui.as_ref().unwrap(),
        next.server_ui.as_ref().unwrap()
    ));
    assert!(next.server_lang.is_some());
    let removed = resource_packs::prepare_changed_application(
        &resource_packs::reuse::CompiledStacks::new(),
        stack(&[]),
        Arc::new(PackInputs::default()),
        Some(&next),
    );
    assert!(removed.server_ui.is_none());
    assert!(removed.server_lang.is_none());
}

/// UI textures load lazily from the pack view, so new pixels at an old path must replace it.
#[test]
fn optional_reload_replaces_ui_whose_texture_pixels_changed() {
    let definition =
        br#"{"namespace":"reload_fixture","panel":{"type":"image","texture":"textures/ui/art"}}"#;
    let first = resource_packs::prepare_validated_application(
        stack(&[
            ("ui/reload.json", definition),
            ("textures/ui/art.png", b"old"),
        ]),
        Arc::new(PackInputs::default()),
    );
    let next = resource_packs::prepare_changed_application(
        &resource_packs::reuse::CompiledStacks::new(),
        stack(&[
            ("ui/reload.json", definition),
            ("textures/ui/art.png", b"new"),
        ]),
        Arc::new(PackInputs::default()),
        Some(&first),
    );
    let view = next.server_ui.as_ref().unwrap().view.as_ref().unwrap();
    assert_eq!(view.read("textures/ui/art.png").unwrap().as_ref(), b"new");
}

#[test]
fn live_reload_removal_releases_old_snapshot_and_keeps_world_identity() {
    let _sounds = client_presentation::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = app();
    let identity = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    let definition = br#"{"namespace":"reload_fixture","panel":{"type":"panel"}}"#;
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[("ui/reload.json", definition)]));
    settle(&mut app, 1);
    let old = Arc::downgrade(app.world().resource::<UiRuntime>().server_ui().unwrap());
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[]));
    settle(&mut app, 2);
    assert!(app.world().resource::<UiRuntime>().server_ui().is_none());
    assert!(
        old.upgrade().is_none(),
        "old presentation snapshot must be released"
    );
    assert_eq!(
        app.world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_session_id(),
        identity
    );
}

#[test]
fn newer_reload_request_supersedes_an_in_flight_worker() {
    let _sounds = client_presentation::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = app();
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[("ui/old.json", br#"{"namespace":"old"}"#)]));
    app.update();
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[("ui/new.json", br#"{"namespace":"new"}"#)]));
    settle(&mut app, 2);
    let ui = app.world().resource::<UiRuntime>().server_ui().unwrap();
    assert_eq!(ui.ui_layers[0][0].0, "ui/new.json");
}

#[test]
#[ignore = "offline large-pack evidence; CINNABAR_RELOAD_PACK names an unencrypted local archive"]
fn large_pack_reload_cpu_benchmark() {
    let _sounds = client_presentation::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = std::env::var_os("CINNABAR_RELOAD_PACK").expect("pack fixture path");
    let dir = std::env::temp_dir().join(format!("cinnabar-reload-bench-{}", std::process::id()));
    let mut version = protocol::GAME_VERSION
        .split('.')
        .map(|part| part.parse().unwrap());
    let mut library = resource_pack::GlobalPackLibrary::open(
        &dir,
        std::array::from_fn(|_| version.next().unwrap_or(0)),
    )
    .unwrap();
    let imported = library.import(std::path::Path::new(&path)).unwrap();
    assert!(
        !imported.imported.is_empty(),
        "fixture must contain an unencrypted resource pack"
    );
    for pack in imported.imported {
        library.activate(pack.id).unwrap();
    }
    let mut app = app();
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(library.apply().unwrap());
    std::fs::remove_dir_all(dir).unwrap();
    let (elapsed, peak) = settle(&mut app, 1);
    let worker = app
        .world()
        .resource::<PackReload>()
        .last_duration()
        .unwrap();
    eprintln!(
        "RESOURCE_RELOAD_CPU swap_ms={:.3} worker_ms={:.3} peak_update_ms={:.3}; diagnostic empty world, no GPU, no reconnect",
        elapsed.as_secs_f64() * 1000.0,
        worker.as_secs_f64() * 1000.0,
        peak.as_secs_f64() * 1000.0
    );
}

/// The actual reload worker publishes pages prepared against the base it installs.
#[test]
fn session_reload_publishes_prepared_actor_pages() {
    let _sounds = client_presentation::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([37, 59, 83, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let packs = resource_packs::prepare_validated_application(
        stack(&[
            ("entity/fixture.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor","geometry":{"default":"geometry.fixture"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/fixture"},"render_controllers":["controller.render.fixture"]}}}"#),
            ("models/entity/fixture.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":1,"texture_height":1},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#),
            ("render_controllers/fixture.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#),
            ("textures/entity/fixture.png", png.get_ref()),
        ]),
        Arc::new(PackInputs::default()),
    );
    let mut app = app();
    app.init_resource::<render::ActorArtworkPages>();
    app.world_mut()
        .resource_mut::<PackReload>()
        .begin_session(1, &packs);
    settle(&mut app, 1);
    assert!(app.world().resource::<PackReload>().error().is_none());
    let world = app.world().resource::<ClientWorld>();
    let pack = world.pack_entities.as_ref().unwrap();
    assert!(!pack.textures.is_empty());
    let base = app.world().resource::<render::ActorArtworkPages>();
    let prepared = world.prepared_actor_artwork.as_ref().unwrap();
    let pages = prepared.pages_for(base, pack).unwrap();
    let expected = base
        .clone()
        .with_pack_artwork(&pack.textures, &pack.bindings);
    assert_eq!(pages.identity(), expected.identity());
    assert_eq!(pages.pages(), expected.pages());
    for binding in pack.bindings.iter() {
        let rig = render_model::pack_rig_id(binding.geometry_candidate);
        assert_eq!(pages.route(rig), expected.route(rig));
    }
}

#[test]
fn aim_highlight_pack_art_is_available_at_join_and_released_at_session_end() {
    let texture = Arc::new(render::AimAssistTexture::new([1, 1], Arc::from([255; 4])).unwrap());
    let packs = resource_packs::PackApplication {
        aim_assist_textures: [Some(texture.clone()), None],
        ..Default::default()
    };
    let mut reload = PackReload::default();
    reload.begin_session(1, &packs);
    assert!(Arc::ptr_eq(
        reload.aim_assist_textures()[0].as_ref().unwrap(),
        &texture
    ));
    reload.end_session();
    assert!(reload.aim_assist_textures().iter().all(Option::is_none));
}
