use super::*;
use pack_runtime::reload_diff::{Changes, Subscriber};

#[test]
fn global_packs_only_recompile_subscribers_with_changed_reads() {
    let server = super::super::pack_reload_tests::stack(&[("texts/en_US.lang", b"server=value")]);
    let joined = resource_packs::prepare_validated_application(server, Arc::default());
    let globals = super::super::pack_reload_tests::stack(&[
        ("texts/en_US.lang", b"global=value"),
        ("extras/global.txt", b"global-only"),
    ]);
    let mut reload = PackReload::default();
    reload.request_globals(globals.clone());
    reload.begin_session(1, &joined);
    let composed = ValidatedPackStack::compose(&globals, &reload.server).unwrap();
    let changes = Changes::between(
        &composed,
        reload
            .previous
            .as_ref()
            .map(|old| (&old.admission, &old.dependencies)),
    );
    let recompiled: Vec<_> = [
        (Subscriber::Blocks, changes.blocks),
        (Subscriber::BlockEntityArt, changes.block_entity_art),
        (Subscriber::Icons, changes.icons),
        (Subscriber::Glyphs, changes.glyphs),
        (Subscriber::Entities, changes.entities),
        (Subscriber::Ui, changes.ui),
        (Subscriber::Sounds, changes.sounds),
        (Subscriber::Language, changes.language),
        (Subscriber::AimAssist, changes.aim_assist),
    ]
    .into_iter()
    .filter_map(|(subscriber, changed)| changed.then_some(subscriber))
    .collect();
    assert_eq!(recompiled, [Subscriber::Sounds, Subscriber::Language]);
    let composed = Arc::new(composed);
    let reused = resource_packs::prepare_diffed_application(
        reload.kept,
        composed.clone(),
        joined.inputs.clone(),
        reload.previous.as_ref().map(|old| (old, changes)),
    );
    let fresh = resource_packs::prepare_validated_application(composed, joined.inputs.clone());
    for key in ["server", "global"] {
        assert_eq!(
            reused
                .server_lang
                .as_ref()
                .and_then(|lang| lang.lookup(key)),
            fresh.server_lang.as_ref().and_then(|lang| lang.lookup(key))
        );
    }
    assert_eq!(reused.terrain_identity, fresh.terrain_identity);
    assert_eq!(reused.dependencies, fresh.dependencies);
}

#[test]
fn first_reload_still_prepares_biomes_and_environment() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(
        assets::TINT_MAP_SIZE,
        assets::TINT_MAP_SIZE,
        image::Rgb([12, 34, 56]),
    )
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let path = format!(
        "textures/colormap/{}.png",
        assets::TintMapId::Grass.source_name()
    );
    let stack = super::super::pack_reload_tests::stack(&[(&path, png.get_ref())]);
    let joined = resource_packs::prepare_validated_application(stack, Arc::default());
    let base = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut reload = PackReload {
        previous_assets: Some(base.clone()),
        ..Default::default()
    };
    reload.begin_session(1, &joined);
    assert!(
        reload.previous_assets.is_none(),
        "a menu carrier cannot stand in for the join"
    );
    reload.start(
        base,
        None,
        Some(EnvironmentBase::new(
            Default::default(),
            None,
            Default::default(),
        )),
        None,
        None,
    );
    let prepared = reload
        .pending
        .as_ref()
        .unwrap()
        .result
        .lock()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(
        &prepared.assets.biome_assets().tint_maps_rgb8[..3],
        &[12, 34, 56]
    );
    for subscriber in [Subscriber::Atmosphere, Subscriber::Particles] {
        assert!(prepared.application.dependencies.contains_key(&subscriber));
    }
    assert!(prepared.application.dependencies[&Subscriber::Blocks].iter().any(|dependency| {
        matches!(dependency, resource_pack::PackDependency::File { path: read, .. } if read == &path)
    }));
}
