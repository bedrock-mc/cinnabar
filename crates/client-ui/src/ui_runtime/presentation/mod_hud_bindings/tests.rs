use super::*;
use crate::{test_support::mini_engine_presentation, ui_runtime::UiRuntime};

/// Creates one bounded numeric global for owner/cache tests.
fn value(name: &str, value: f64) -> HudBindings {
    HudBindings(BTreeMap::from([(name.into(), SurfaceValue::Number(value))]))
}

#[test]
fn owners_merge_without_mutating_native_data_and_clear_independently() {
    let mut native = DataSource::new();
    native.set_global("#native", Scalar::Bool(true));
    let base = Arc::new(native.clone());
    let mut bindings = Bindings::default();
    let environment = PanelUiOwner::Environment(1);
    let packet = PanelUiOwner::PacketSession(2);
    bindings
        .set(environment, Some(value("#mod_shared", 1.0)))
        .unwrap();
    bindings
        .set(packet, Some(value("#mod_shared", 2.0)))
        .unwrap();
    let merged = bindings.merge(Arc::clone(&base), false);
    let mut expected = native.clone();
    expected.set_global("#mod_shared", Scalar::Num(2.0));
    assert_eq!(*merged, expected);
    assert_eq!(*base, native);
    assert!(Arc::ptr_eq(
        &merged,
        &bindings.merge(Arc::clone(&base), false)
    ));
    bindings.set(packet, None).unwrap();
    expected.set_global("#mod_shared", Scalar::Num(1.0));
    assert_eq!(*bindings.merge(Arc::clone(&base), false), expected);
    bindings.set(environment, None).unwrap();
    assert!(Arc::ptr_eq(
        &base,
        &bindings.merge(Arc::clone(&base), false)
    ));
}

#[test]
fn idle_values_reuse_both_native_bases_and_invalid_updates_leave_the_overlay_intact() {
    let hud = Arc::new(DataSource::new());
    let cursor = Arc::new(DataSource::new());
    let owner = PanelUiOwner::PacketSession(1);
    let mut bindings = Bindings::default();
    bindings.set(owner, Some(value("#mod_scale", 1.0))).unwrap();
    let hud_overlay = bindings.merge(Arc::clone(&hud), false);
    let cursor_overlay = bindings.merge(Arc::clone(&cursor), true);
    bindings.set(owner, Some(value("#mod_scale", 1.0))).unwrap();
    assert!(bindings.set(owner, Some(value("#native", 2.0))).is_err());
    assert!(Arc::ptr_eq(
        &hud_overlay,
        &bindings.merge(Arc::clone(&hud), false)
    ));
    assert!(Arc::ptr_eq(&cursor_overlay, &bindings.merge(cursor, true)));
    let mut changed = (*hud).clone();
    changed.set_global("#native_new", Scalar::Bool(true));
    let changed = Arc::new(changed);
    assert!(!Arc::ptr_eq(&hud_overlay, &bindings.merge(changed, false)));
}

#[test]
fn trusted_hud_catalog_is_cached_and_the_last_owner_restores_the_original() {
    let base = Arc::new(Catalog::default());
    let mut bindings = Bindings::default();
    assert!(Arc::ptr_eq(&base, &bindings.catalog(&base)));
    let owner = PanelUiOwner::PacketSession(1);
    bindings.set(owner, Some(value("#mod_scale", 1.0))).unwrap();
    let opted = bindings.catalog(&base);
    assert!(opted.paint_transforms());
    assert!(!base.paint_transforms());
    assert!(Arc::ptr_eq(&opted, &bindings.catalog(&base)));
    bindings.set(owner, Some(value("#mod_scale", 2.0))).unwrap();
    assert!(Arc::ptr_eq(&opted, &bindings.catalog(&base)));
    bindings.set(owner, None).unwrap();
    assert!(Arc::ptr_eq(&base, &bindings.catalog(&base)));
}

#[test]
fn real_pack_hud_consumes_globals_while_the_menu_is_closed_and_reuses_native_semantics() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "namespace":"hud","hud_screen":{"type":"screen","controls":[{"fixture":{
                        "type":"label","text":"HUD","size":[40,12],"offset":[20,30],
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "cinnabar_transform":{"origin":[0,0]},"bindings":[
                            {"binding_name":"#mod_position","binding_name_override":"#offset"},
                            {"binding_name":"#mod_scale","binding_name_override":"#cinnabar_scale"}
                        ]
                    }}]}
                }))
                .unwrap(),
            ),
        ]],
        ..Default::default()
    });
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let runtime = UiRuntime::new(1);
    let build = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 800],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    let baseline = build(&mut presentation);
    let owner = PanelUiOwner::PacketSession(1);
    let globals = HudBindings(BTreeMap::from([
        (
            "#mod_position".into(),
            SurfaceValue::Vector(vec![100.0, 80.0]),
        ),
        ("#mod_scale".into(), SurfaceValue::Number(1.5)),
    ]));
    presentation
        .set_mod_hud_bindings(owner, Some(globals))
        .unwrap();
    let moved = build(&mut presentation);
    assert!(!presentation.mod_ui().mod_panel_open());
    let node = presentation
        .hud_draw_nodes()
        .iter()
        .find(|node| node.name == "fixture")
        .expect("pack HUD painted");
    assert_eq!([node.dest.x, node.dest.y], [100.0, 80.0]);
    assert_ne!(baseline.vertices, moved.vertices);
    let constructions = presentation.hud_component_constructions();
    let unscaled = HudBindings(BTreeMap::from([
        (
            "#mod_position".into(),
            SurfaceValue::Vector(vec![100.0, 80.0]),
        ),
        ("#mod_scale".into(), SurfaceValue::Number(1.0)),
    ]));
    presentation
        .set_mod_hud_bindings(owner, Some(unscaled))
        .unwrap();
    assert_ne!(
        moved.vertices,
        build(&mut presentation).vertices,
        "the actual HUD painter honors its trusted scale binding"
    );
    assert_eq!(presentation.hud_component_constructions(), constructions);
    let passes = presentation.hud_passes();
    build(&mut presentation);
    assert_eq!(presentation.hud_component_constructions(), constructions);
    assert_eq!(presentation.hud_passes(), passes);
    presentation.set_mod_hud_bindings(owner, None).unwrap();
    let restored = build(&mut presentation);
    assert_eq!(baseline.vertices, restored.vertices);
}
