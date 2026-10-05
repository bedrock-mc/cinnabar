use resource_pack::{AdmissionError, PackAdmission};

use super::ResourcePackAdmissionState;

/// Serializes tests that go through the process-wide block overlay cache.
static OVERLAY_CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn overlay_cache() -> std::sync::MutexGuard<'static, ()> {
    OVERLAY_CACHE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn absent_or_rejected_application_preserves_optional_admission() {
    let _cache = overlay_cache();
    let application = super::prepare_pack_application(
        protocol::ResourcePackHandoff::default(),
        &protocol::CustomBlocks::default(),
        &[],
        &[],
        false,
    );
    assert!(matches!(application.admission, PackAdmission::None));
    assert!(application.server_lang.is_none());
    let pack = protocol::ResourcePackArchive::unencrypted(
        "11111111-2222-3333-4444-555555555555".parse().unwrap(),
        "1.2.3".into(),
        String::new(),
        vec![0; 32],
    );
    let application = super::prepare_pack_application(
        protocol::ResourcePackHandoff::from_archives(vec![pack]),
        &protocol::CustomBlocks::default(),
        &[],
        &[],
        false,
    );
    let overlay = application.server_lang;
    let PackAdmission::Validated(stack) = application.admission else {
        panic!("a dropped pack still yields an admitted stack");
    };
    assert!(stack.packs().is_empty());
    assert_eq!(
        stack.rejections()[0].reason,
        AdmissionError::InvalidZipFooter
    );
    assert!(overlay.is_none());
}

// A required pack that fails validation refuses the join in vanilla's words;
// an optional one is dropped and the join goes on.
#[test]
fn a_rejected_required_pack_refuses_the_join() {
    let _cache = overlay_cache();
    for required in [false, true] {
        let broken = protocol::ResourcePackArchive::unencrypted(
            "11111111-2222-3333-4444-555555555555".parse().unwrap(),
            "1.2.3".into(),
            String::new(),
            vec![0; 32],
        );
        let handoff =
            protocol::ResourcePackHandoff::from_archives(vec![broken]).with_required(required);
        assert_eq!(handoff.required(), required);
        let application = super::prepare_pack_application(
            handoff,
            &protocol::CustomBlocks::default(),
            &[],
            &[],
            false,
        );
        let outcome = super::required_packs_applied(required, &application.admission);
        assert_eq!(outcome.is_err(), required);
        if let Err(error) = outcome {
            let failure =
                crate::runtime::network::session_failure_display(&error.to_string(), None);
            assert_eq!(
                crate::menu::disconnect::describe(&failure).body,
                crate::menu::disconnect::DisconnectBody::Key("disconnectionScreen.resourcePack")
            );
        }
    }
}

fn lang_pack(id: u128, lang: &[u8]) -> protocol::ResourcePackArchive {
    archive(id, &[("texts/en_US.lang", lang)])
}

fn archive(id: u128, files: &[(&str, &[u8])]) -> protocol::ResourcePackArchive {
    use std::io::Write;
    let id = format!("00000000-0000-0000-0000-{id:012x}");
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let archive = writer.finish().unwrap().into_inner();
    protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        archive,
    )
}

fn files_pack(files: &[(&str, &[u8])]) -> resource_pack::LayeredPackView {
    use std::io::Write;
    let id = "00000000-0000-0000-0000-00000000000a";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    );
    resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

#[test]
fn ui_index_loads_custom_paths_and_extensions() {
    let path = "custom/inventory.uidx";
    let view = files_pack(&[
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/inventory.uidx"]}"#,
        ),
        (
            path,
            br#"{"namespace":"custom","inventory":{"type":"panel"}}"#,
        ),
        ("custom/unlisted.uidx", b"unlisted data"),
    ]);
    let pack = super::collect_server_ui(&view).unwrap();
    assert!(pack.ui_layers[0].iter().any(|(name, _)| name == path));
    assert!(
        !pack.ui_layers[0]
            .iter()
            .any(|(name, _)| name == "custom/unlisted.uidx")
    );
    let mut catalog = json_ui::Catalog::default();
    catalog.apply_pack(
        pack.ui_layers[0]
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
    );
    assert!(catalog.lookup("custom", "inventory").is_some());
}

#[test]
fn ui_custom_definition_overrides_across_layers_without_relisting() {
    let path = "custom/inventory.uidx";
    let lower = archive(
        1,
        &[
            (
                "ui/_ui_defs.json",
                br#"{"ui_defs":["custom/inventory.uidx"]}"#,
            ),
            (
                path,
                br#"{"namespace":"custom","inventory":{"type":"panel","size":[10,10]}}"#,
            ),
        ],
    );
    let upper = archive(
        2,
        &[(
            path,
            br#"{"namespace":"custom","inventory":{"size":[20,20]}}"#,
        )],
    );
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![lower, upper]),
    ));
    let pack = super::collect_server_ui(&view).unwrap();
    assert!(pack.ui_layers[1].iter().any(|(name, _)| name == path));
    let mut catalog = json_ui::Catalog::default();
    for files in &pack.ui_layers {
        catalog.apply_pack(
            files
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        );
    }
    assert_eq!(
        catalog.lookup("custom", "inventory").unwrap().props["size"],
        serde_json::json!([20, 20])
    );
}

#[test]
fn ui_reload_tracks_declared_documents_and_external_art() {
    use super::super::pack_reload_diff::{Changes, Subscriber, compile};
    for changed in [
        "custom/inventory.uidx",
        "assets/gui/inventory.png",
        "assets/gui/inventory.json",
    ] {
        let inputs = |value| {
            files_pack(&[
                (
                    "ui/_ui_defs.json",
                    br#"{"ui_defs":["custom/inventory.uidx"]}"#,
                ),
                (
                    "custom/inventory.uidx",
                    if changed == "custom/inventory.uidx" {
                        value
                    } else {
                        b"{}"
                    },
                ),
                (
                    "assets/gui/inventory.png",
                    if changed == "assets/gui/inventory.png" {
                        value
                    } else {
                        b"pixels"
                    },
                ),
                (
                    "assets/gui/inventory.json",
                    if changed == "assets/gui/inventory.json" {
                        value
                    } else {
                        b"{}"
                    },
                ),
            ])
        };
        let before = inputs(b"before");
        let mut dependencies = Default::default();
        compile(
            Subscriber::Ui,
            &before.shared_stack(),
            &mut dependencies,
            super::collect_server_ui,
        );
        let previous = super::PackApplication {
            admission: resource_pack::PackAdmission::Validated(before.shared_stack()),
            dependencies,
            ..Default::default()
        };
        assert!(
            Changes::between(inputs(b"after").stack(), Some(&previous)).ui,
            "{changed}"
        );
    }
}

#[test]
fn ui_reload_tracks_new_art_without_reloading_for_unrelated_files() {
    use super::super::pack_reload_diff::{Changes, Subscriber, compile};
    let before = files_pack(&[("assets/gui/inventory.png", b"pixels")]);
    let mut dependencies = Default::default();
    compile(
        Subscriber::Ui,
        &before.shared_stack(),
        &mut dependencies,
        super::collect_server_ui,
    );
    let previous = super::PackApplication {
        admission: resource_pack::PackAdmission::Validated(before.shared_stack()),
        dependencies,
        ..Default::default()
    };
    for (added, changed) in [("assets/gui/new.png", true), ("texts/en_US.lang", false)] {
        let after = files_pack(&[("assets/gui/inventory.png", b"pixels"), (added, b"new")]);
        assert_eq!(
            Changes::between(after.stack(), Some(&previous)).ui,
            changed,
            "{added}"
        );
    }
}

// A cancelled session's late preparation must not replace the live session's sounds.
#[test]
fn preparing_packs_leaves_sound_publication_to_bootstrap() {
    let _cache = overlay_cache();
    let _mailbox = crate::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = crate::audio::server_sounds_generation();
    let sounds = br#"{"sound_definitions":{"custom.beep":{"sounds":["sounds/beep"]}}}"#;
    let application = super::prepare_pack_application(
        protocol::ResourcePackHandoff::from_archives(vec![archive(
            3,
            &[("sounds/sound_definitions.json", sounds)],
        )]),
        &protocol::CustomBlocks::default(),
        &[],
        &[],
        false,
    );
    assert_eq!(crate::audio::server_sounds_generation(), before);
    assert!(application.server_sounds.is_some(), "carried to Bootstrap");
}

// A texture set resolves to its sibling color image or a solid color.
#[test]
fn texture_sets_supply_color_layers() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    let view = files_pack(&[
        (
            "textures/blocks/a.texture_set.json",
            br#"{"format_version":"1.16.100","minecraft:texture_set":{"color":"a_color"}}"#,
        ),
        ("textures/blocks/a_color.png", &png),
        (
            "textures/blocks/b.texture_set.json",
            br#"{"minecraft:texture_set":{"color":[10,20,30]}}"#,
        ),
    ]);
    let image = super::decode_pack_texture(&view, "textures/blocks/a").expect("sibling");
    assert_eq!(
        (image.width, image.height, image.rgba8[..4].to_vec()),
        (2, 2, vec![1, 2, 3, 255])
    );
    let solid = super::decode_pack_texture(&view, "textures/blocks/b").expect("solid");
    assert_eq!(solid.rgba8.to_vec(), vec![10, 20, 30, 255]);
}

// Higher packs override shared keys; keys only a lower pack defines survive.
#[test]
fn language_files_merge_across_the_stack_by_precedence() {
    let _cache = overlay_cache();
    let handoff = protocol::ResourcePackHandoff::from_archives(vec![
        lang_pack(2, b"\xef\xbb\xbfshared=bottom\nbottom.only=B"),
        lang_pack(1, b"shared=top\ntop.only=T"),
    ]);
    let application = super::prepare_pack_application(
        handoff,
        &protocol::CustomBlocks::default(),
        &[],
        &[],
        false,
    );
    let overlay = application.server_lang.expect("merged overlay");
    assert_eq!(overlay.lookup("shared"), Some("top"));
    assert_eq!(overlay.lookup("top.only"), Some("T"));
    assert_eq!(overlay.lookup("bottom.only"), Some("B"));
}

// The same stack and blocks reuse the compiled overlay instead of recompiling.
#[test]
fn overlay_cache_reuses_the_previous_session_compile() {
    let _cache = overlay_cache();
    let blocks = protocol::CustomBlocks {
        blocks: vec![protocol::CustomBlock {
            name: "cache:test".into(),
            state_count: 1,
            collides: true,
            collision_box: None,
            selection: Default::default(),
            visual: Default::default(),
        }]
        .into(),
        vanilla_blocks: Default::default(),
        skipped: 0,
    };
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            lang_pack(7, b"a=b"),
        ]));
    let mut compiles = 0;
    let fingerprint = super::stack_fingerprint(&stack);
    let view = resource_pack::LayeredPackView::tracked(stack);
    for _ in 0..2 {
        super::cached_block_overlay(&fingerprint, &view, &blocks, false, || {
            compiles += 1;
            None
        });
    }
    assert_eq!(compiles, 1);
    super::cached_block_overlay(&fingerprint, &view, &blocks, true, || {
        compiles += 1;
        None
    });
    assert_eq!(compiles, 2);
}

#[test]
fn newer_generation_replaces_atomically_and_stale_results_are_ignored() {
    let mut state = ResourcePackAdmissionState::default();
    assert!(state.begin_generation(2));
    assert!(matches!(state.admission(), PackAdmission::None));
    let stack = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
    assert!(state.replace_for_generation(2, PackAdmission::Validated(stack)));
    assert!(!state.replace_for_generation(1, PackAdmission::None));
    assert_eq!(state.generation(), 2);
    assert!(matches!(state.admission(), PackAdmission::Validated(_)));
    assert!(state.begin_generation(3));
    assert!(matches!(state.admission(), PackAdmission::None));
    assert!(!state.begin_generation(2));
    state.clear_current();
    assert_eq!(state.generation(), 3);
    assert!(matches!(state.admission(), PackAdmission::None));
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    png.into_inner()
}

/// A stack exercising every subscriber; `id` keeps each copy out of the in-memory caches.
fn every_subscriber(id: u128) -> std::sync::Arc<resource_pack::ValidatedPackStack> {
    let (texture, sheet) = (png(1, 1), png(128, 128));
    resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![archive(
        id,
        &[
            ("texts/en_US.lang", b"a=b"),
            ("ui/_ui_defs.json", br#"{"ui_defs":["ui/x.json"]}"#),
            ("ui/x.json", br#"{"namespace":"x","c":{"type":"label"}}"#),
            ("sounds/sound_definitions.json", br#"{"sound_definitions":{"x.beep":{"sounds":["sounds/beep"]}}}"#),
            ("textures/item_texture.json", br#"{"texture_data":{"gem":{"textures":"textures/items/gem"}}}"#),
            ("textures/items/gem.png", &texture),
            ("font/glyph_00.png", &sheet),
            ("entity/fixture.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor","geometry":{"default":"geometry.fixture"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/fixture"},"render_controllers":["controller.render.fixture"]}}}"#),
            ("models/entity/fixture.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":1,"texture_height":1},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#),
            ("render_controllers/fixture.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#),
            ("textures/entity/fixture.png", &texture),
        ],
    )]))
}

fn every_input() -> std::sync::Arc<super::super::pack_reload::PackInputs> {
    std::sync::Arc::new(super::super::pack_reload::PackInputs {
        icons: vec![("x:gem".into(), "gem".into())],
        ..Default::default()
    })
}

/// Everything a subscriber produced, in a deterministic form.
fn summary(application: &super::PackApplication) -> String {
    let icons = application.item_icons.as_ref().map(|icons| {
        icons
            .icons
            .iter()
            .map(|icon| (icon.identifier.clone(), icon.metadata, icon.rgba8.clone()))
            .collect::<Vec<_>>()
    });
    let glyphs = application
        .glyph_sheets
        .as_ref()
        .map(|glyphs| format!("{:?} {:?}", glyphs.cells, glyphs.named));
    let entities = application
        .entities
        .as_ref()
        .map(|pack| format!("{:?} {:?} {:?}", pack.assets, pack.textures, pack.bindings));
    format!(
        "{:?}\n{:?}\n{icons:?}\n{glyphs:?}\n{entities:?}\n{:?}\n{}",
        application.dependencies,
        application
            .server_lang
            .as_ref()
            .and_then(|lang| lang.lookup("a")),
        application
            .server_ui
            .as_ref()
            .map(|ui| format!("{:?}", ui.ui_layers)),
        application.server_sounds.is_some(),
    )
}

// Subscribers compiled concurrently produce exactly what one thread compiling them in turn does.
#[test]
fn parallel_preparation_matches_a_serial_one() {
    let _cache = overlay_cache();
    let serial = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| super::prepare_validated_application(every_subscriber(21), every_input()));
    let parallel = super::prepare_validated_application(every_subscriber(22), every_input());
    for application in [&serial, &parallel] {
        assert!(application.server_lang.is_some() && application.item_icons.is_some());
        assert!(application.entities.is_some() && application.server_ui.is_some());
    }
    assert_eq!(summary(&serial), summary(&parallel));
}

// Cancellation that lands while a part compiles stops every later part and yields nothing.
#[test]
fn cancellation_mid_compile_skips_the_remaining_parts() {
    let _cache = overlay_cache();
    let polls = std::sync::atomic::AtomicUsize::new(0);
    let cancel_after_first = || polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 0;
    let prepared = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            super::compile_application(
                every_subscriber(23),
                every_input(),
                None,
                &cancel_after_first,
            )
        });
    assert!(prepared.is_none());
    assert!(
        super::compile_application(every_subscriber(24), every_input(), None, &|| true).is_none()
    );
}
