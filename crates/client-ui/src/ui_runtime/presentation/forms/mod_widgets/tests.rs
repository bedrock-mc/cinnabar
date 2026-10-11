use super::super::{ServerUiPack, snapshot, tests::mini_engine_presentation};
use super::*;
use ui::{
    DpiScale,
    mod_hud::{Anchor, Card, Row},
};

/// Loads the same real font, HUD, item icons and JSON-UI carriers used by the client.
pub(in super::super) fn installed_hud_presentation() -> Option<UiPresentationRuntime> {
    use assets::{RuntimeHudCatalog, RuntimeIconCatalog};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local");
    let read = |name: &str| {
        let path = root.join("assets/compiled").join(name);
        match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "skipping installed personal HUD fixture: missing {}; make assets",
                    path.display()
                );
                None
            }
            Err(error) => panic!("read personal HUD fixture {}: {error}", path.display()),
        }
    };
    let hud = Arc::new(
        RuntimeHudCatalog::decode(&read(assets::carriers::HUD.output)?)
            .expect("decode installed HUD fixture"),
    );
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&read(assets::carriers::ICON.output)?)
            .expect("decode installed item icon fixture"),
    );
    let font = Arc::new(
        (*super::super::pack_harness::font())
            .clone()
            .with_coverage_pages(),
    );
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(font, hud, icons)
        .expect("build installed HUD fixture");
    presentation
        .enable_json_ui(super::super::pack_harness::carrier()?)
        .expect("enable installed JSON-UI fixture");
    presentation
        .form_presentation
        .engine
        .as_mut()
        .unwrap()
        .textures
        .set_fallbacks(
            Default::default(),
            root.join(launcher::install_layout::vanilla_pack_relative()),
        );
    Some(presentation)
}

pub(in super::super) fn content() -> Hud {
    Hud {
        cards: vec![Card {
            id: "equipment".into(),
            title: "Equipment".into(),
            anchor: Anchor::TopLeft,
            offset: [6., 6.],
            scale: 1.,
            rows: vec![Row {
                label: "Helmet".into(),
                value: "85%".into(),
                item: Some("minecraft:diamond_helmet".into()),
                effect_id: None,
                metadata: 0,
                progress: Some(0.85),
                color: [0.3, 1., 0.5, 1.],
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}
pub(in super::super) fn presentation(hidden: bool) -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    let hud = serde_json::json!({"namespace":"hud","hud_screen":{"type":"screen","visible":!hidden,
        "controls":[{"label":{"type":"label","size":[100,12],"text":"Base HUD","anchor_from":"bottom_middle","anchor_to":"bottom_middle"}}]}});
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                serde_json::to_vec(&hud).unwrap(),
            ),
        ]],
        ..Default::default()
    });
    presentation
}
pub(in super::super) fn frame(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    size: [u32; 2],
    dpi: f32,
) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            0,
            size,
            DpiScale::new(dpi).unwrap(),
        )
        .unwrap()
}
#[test]
fn cards_update_values_without_rebuilding_the_json_ui_catalog_and_revoke_cleanly() {
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let before = frame(&mut p, &runtime, [1280, 720], 1.);
    let mut hud = content();
    p.set_mod_hud(Some(&hud)).unwrap();
    let after = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&before) != snapshot::rasterize(&after),
        "published cards change the rendered frame"
    );
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    assert!(
        after == frame(&mut p, &runtime, [1280, 720], 1.),
        "unchanged content reuses the published frame"
    );
    hud.cards[0].rows[0].value = "41%".into();
    hud.cards[0].rows[0].progress = Some(0.41);
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(Arc::ptr_eq(
        &catalog,
        &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
    ));
    let updated = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&after) != snapshot::rasterize(&updated),
        "new values change the rendered cards"
    );
    p.set_mod_hud(None).unwrap();
    let restored = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&before) == snapshot::rasterize(&restored),
        "revoking cards restores the original pixels"
    );
}
#[test]
fn changed_card_layout_rebuilds_the_catalog_and_stacks_rendered_text() {
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let mut hud = content();
    hud.cards[0].title.clear();
    hud.cards[0].rows[0].item = None;
    hud.cards[0].rows[0].progress = None;
    hud.cards[0].rows[0].color = [1., 0., 0., 1.];
    p.set_mod_hud(Some(&hud)).unwrap();
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    let standard = frame(&mut p, &runtime, [1280, 720], 1.);
    hud.cards[0].row_layout = ui::mod_hud::RowLayout::StackedText;
    hud.cards[0].width = 128.;
    hud.cards[0].row_height = 34.;
    hud.cards[0].icon_size = 22.;
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(!Arc::ptr_eq(
        &catalog,
        &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
    ));
    let stacked = frame(&mut p, &runtime, [1280, 720], 1.);
    assert_ne!(
        snapshot::rasterize(&standard),
        snapshot::rasterize(&stacked)
    );
    let nodes = &p.last_frame.as_ref().unwrap().nodes;
    // Node bounds are parent-local; compare the row runs in the card's world space.
    let world_bounds = |node: &ui::UiNode| {
        let bounds = node.bounds();
        let mut offset = [0.; 2];
        let mut parent = node.parent();
        while let Some(id) = parent {
            let ancestor = nodes.iter().find(|node| node.id() == id).unwrap();
            offset[0] += ancestor.bounds().min().x();
            offset[1] += ancestor.bounds().min().y();
            parent = ancestor.parent();
        }
        ui::UiRect::new(
            ui::UiPoint::new(bounds.min().x() + offset[0], bounds.min().y() + offset[1]).unwrap(),
            ui::UiPoint::new(bounds.max().x() + offset[0], bounds.max().y() + offset[1]).unwrap(),
        )
        .unwrap()
    };
    let surface = nodes
        .iter()
        .find_map(|node| {
            let ui::UiVisual::Mesh(mesh) = node.visual() else {
                return None;
            };
            mesh.vertices()
                .iter()
                .any(|vertex| vertex.color == [11, 13, 17, 209])
                .then(|| world_bounds(node))
        })
        .expect("rendered card surface");
    let text_bounds = |color| {
        let matching: Vec<_> = nodes
            .iter()
            .filter_map(|node| {
                matches!(node.visual(), ui::UiVisual::Text {color:actual,..} if *actual == color)
                    .then(|| world_bounds(node))
                    .filter(|bounds| {
                        bounds.min().x() >= surface.min().x()
                            && bounds.max().x() <= surface.max().x()
                            && bounds.min().y() >= surface.min().y()
                            && bounds.max().y() <= surface.max().y()
                    })
            })
            .collect();
        assert_eq!(matching.len(), 1, "one rendered row run inside the card");
        matching[0]
    };
    let name = text_bounds([255; 4]);
    let value = text_bounds([255, 0, 0, 255]);
    assert!(
        name.max().y() <= value.min().y(),
        "stacked text uses distinct lines"
    );
    assert_eq!(name.min().x(), value.min().x());
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    hud.cards[0].rows[0].value = "37%".into();
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(
        Arc::ptr_eq(
            &catalog,
            &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
        ),
        "a changing value retains the selected row layout"
    );
}
#[test]
fn larger_row_text_rebuilds_the_catalog_and_fits_stacked_and_right_icon_bands() {
    for (layout, width, height, icon) in [
        (ui::mod_hud::RowLayout::StackedText, 128., 34., 22.),
        (ui::mod_hud::RowLayout::IconRight, 72., 28., 20.),
    ] {
        let mut p = presentation(false);
        let mut hud = content();
        let card = &mut hud.cards[0];
        card.title.clear();
        card.row_layout = layout;
        card.width = width;
        card.row_height = height;
        card.icon_size = icon;
        card.rows[0].progress = None;
        card.rows[0].value = "1234".into();
        card.rows[0].color = [1., 0., 0., 1.];
        if layout == ui::mod_hud::RowLayout::IconRight {
            card.rows[0].label.clear();
        }
        let dimensions = template::dimensions(card);
        p.set_mod_hud(Some(&hud)).unwrap();
        let baseline = frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        let baseline_nodes = &p.last_frame.as_ref().unwrap().nodes;
        let (_, baseline_value) = contained_row_text(
            baseline_nodes,
            rendered_card_surface(baseline_nodes),
            [255, 0, 0, 255],
        );
        let base_scale = baseline_value.key().scale_1024;
        let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
        hud.cards[0].text_scale = 1.75;
        assert_eq!(template::dimensions(&hud.cards[0]), dimensions);
        p.set_mod_hud(Some(&hud)).unwrap();
        assert!(!Arc::ptr_eq(
            &catalog,
            &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
        ));
        let enlarged = frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
        assert_ne!(
            snapshot::rasterize(&baseline),
            snapshot::rasterize(&enlarged)
        );
        let nodes = &p.last_frame.as_ref().unwrap().nodes;
        let surface = rendered_card_surface(nodes);
        let (value, value_layout) = contained_row_text(nodes, surface, [255, 0, 0, 255]);
        assert!(value_layout.key().scale_1024 > base_scale);
        assert_eq!(
            value_layout.line_count(),
            1,
            "four digits fit the value band"
        );
        assert!(value.min().y() >= surface.min().y());
        assert!(value.max().y() <= surface.max().y());
        let glyph_bottom = value_layout
            .glyphs()
            .iter()
            .map(|glyph| glyph.bounds_64[3])
            .max()
            .expect("value glyphs") as f32
            / 64.;
        assert!(
            glyph_bottom <= value.max().y() - value.min().y() + 0.01,
            "layout={layout:?}, glyph_bottom={glyph_bottom}, band={value:?}"
        );
        if layout == ui::mod_hud::RowLayout::StackedText {
            let (name, _) = contained_row_text(nodes, surface, [255; 4]);
            assert!(name.max().y() <= value.min().y());
        }
    }
}

#[test]
fn right_icon_values_stay_centered_when_text_and_hud_are_scaled() {
    for scale in [0.5, 0.75, 1., 1.5, 2.] {
        for text_scale in [1., 1.75] {
            let mut p = presentation(false);
            let mut hud = content();
            let card = &mut hud.cards[0];
            card.title.clear();
            card.row_layout = ui::mod_hud::RowLayout::IconRight;
            card.width = 72.;
            card.row_height = 28.;
            card.icon_size = 20.;
            card.scale = scale;
            card.text_scale = text_scale;
            let row = &mut card.rows[0];
            row.label.clear();
            row.value = "363".into();
            row.progress = None;
            row.color = [1., 0., 0., 1.];
            p.set_mod_hud(Some(&hud)).unwrap();
            frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
            let nodes = &p.last_frame.as_ref().unwrap().nodes;
            let surface = rendered_card_surface(nodes);
            let (bounds, layout) = contained_row_text(nodes, surface, [255, 0, 0, 255]);
            assert_eq!(layout.line_count(), 1);
            let text_center = (bounds.min().y() + bounds.max().y()) * 0.5;
            let pixels_per_unit = (surface.max().x() - surface.min().x()) / (72. * scale);
            let row_center = surface.min().y() + 14. * scale * pixels_per_unit;
            assert!(
                // Text and icon origins snap independently to whole output pixels.
                (text_center - row_center).abs() <= 1.,
                "scale={scale}, text_scale={text_scale}: text={text_center}, icon row={row_center}"
            );
        }
    }
}

#[test]
fn replacement_effect_icons_hide_and_restore_without_changing_player_effects() {
    let mut p = presentation(false);
    let definition = serde_json::json!({"namespace":"hud", "hud_screen":{
        "type":"screen", "controls":[
            {"effects":{"type":"custom", "renderer":"mob_effects_renderer",
                "size":[100,50], "anchor_from":"top_right", "anchor_to":"top_right"}},
            {"visible_marker":{"type":"label", "text":"Effects", "size":[100,12],
                "bindings":[{"binding_name":"#status_effects_visible", "binding_name_override":"#visible"}]}}
        ]
    }});
    p.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                serde_json::to_vec(&definition).unwrap(),
            ),
        ]],
        ..Default::default()
    });
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(
            1,
            1,
            protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: protocol::ActorEffectAction::Add,
                effect_id: 1,
                amplifier: 0,
                particles: true,
                ambient: false,
                duration_ticks: -1,
                tick: 0,
            },
            0,
        )
        .unwrap();
    let ordinary = snapshot::rasterize(&frame(&mut p, &runtime, [1280, 720], 1.));
    let right_icons = |nodes: &[ui::UiNode]| {
        nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.visual(),
                    ui::UiVisual::Sprite { .. }
                        | ui::UiVisual::StyledSprite { .. }
                        | ui::UiVisual::Solid { .. }
                ) && composed_bounds(nodes, node).min().x() > 1000.
            })
            .count()
    };
    let nodes = &p.last_frame.as_ref().unwrap().nodes;
    assert_eq!(
        right_icons(nodes),
        2,
        "ordinary icon and its background are drawn"
    );
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
    );
    let mut replacement = Hud {
        hide_effect_icons: true,
        ..Default::default()
    };
    p.set_mod_hud(Some(&replacement)).unwrap();
    let hidden = snapshot::rasterize(&frame(&mut p, &runtime, [1280, 720], 1.));
    assert!(
        ordinary != hidden,
        "both ordinary icon art and its visibility binding disappear"
    );
    let nodes = &p.last_frame.as_ref().unwrap().nodes;
    assert_eq!(
        right_icons(nodes),
        0,
        "an unconditional native effect renderer is empty"
    );
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Text { .. })),
        "the status-effect visibility binding is false"
    );
    assert_eq!(runtime.gameplay_hud().effects().len(), 1);
    replacement.hide_effect_icons = false;
    p.set_mod_hud(Some(&replacement)).unwrap();
    assert!(ordinary == snapshot::rasterize(&frame(&mut p, &runtime, [1280, 720], 1.)));
    replacement.hide_effect_icons = true;
    p.set_mod_hud(Some(&replacement)).unwrap();
    p.set_mod_hud(None).unwrap();
    assert!(
        ordinary == snapshot::rasterize(&frame(&mut p, &runtime, [1280, 720], 1.)),
        "revoking the component restores ordinary icons"
    );
}

/// Locates the card surface in the same world coordinates as its row text.
fn rendered_card_surface(nodes: &[ui::UiNode]) -> ui::UiRect {
    nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Mesh(mesh)
                if mesh
                    .vertices()
                    .iter()
                    .any(|vertex| vertex.color == [11, 13, 17, 209]) =>
            {
                Some(composed_bounds(nodes, node))
            }
            _ => None,
        })
        .expect("rendered card surface")
}

/// Requires exactly one matching row run inside the card's full world rectangle.
fn contained_row_text(
    nodes: &[ui::UiNode],
    surface: ui::UiRect,
    color: [u8; 4],
) -> (ui::UiRect, &ui::TextLayout) {
    let matches: Vec<_> = nodes
        .iter()
        .filter_map(|node| {
            let ui::UiVisual::Text {
                layout,
                color: actual,
                ..
            } = node.visual()
            else {
                return None;
            };
            let bounds = composed_bounds(nodes, node);
            (*actual == color
                && bounds.min().x() >= surface.min().x()
                && bounds.max().x() <= surface.max().x()
                && bounds.min().y() >= surface.min().y()
                && bounds.max().y() <= surface.max().y())
            .then_some((bounds, layout.as_ref()))
        })
        .collect();
    assert_eq!(matches.len(), 1, "one matching row run inside the card");
    matches[0]
}

/// Composes ancestor offsets before comparing independently parented render nodes.
fn composed_bounds(nodes: &[ui::UiNode], node: &ui::UiNode) -> ui::UiRect {
    let bounds = node.bounds();
    let mut offset = [0.; 2];
    let mut parent = node.parent();
    while let Some(id) = parent {
        let ancestor = nodes.iter().find(|node| node.id() == id).unwrap();
        offset[0] += ancestor.bounds().min().x();
        offset[1] += ancestor.bounds().min().y();
        parent = ancestor.parent();
    }
    ui::UiRect::new(
        ui::UiPoint::new(bounds.min().x() + offset[0], bounds.min().y() + offset[1]).unwrap(),
        ui::UiPoint::new(bounds.max().x() + offset[0], bounds.max().y() + offset[1]).unwrap(),
    )
    .unwrap()
}

#[test]
fn compact_inline_rows_keep_progress_separate_from_rendered_text() {
    for layout in [
        ui::mod_hud::RowLayout::Standard,
        ui::mod_hud::RowLayout::IconRight,
    ] {
        let maximum = if layout == ui::mod_hud::RowLayout::Standard {
            20
        } else {
            23
        };
        for height in 12..=maximum {
            let mut p = presentation(false);
            let mut hud = content();
            let card = &mut hud.cards[0];
            card.title.clear();
            card.row_layout = layout;
            card.row_height = height as f32;
            card.icon_size = 4.;
            card.rows[0].item = None;
            card.rows[0].color = [1., 0., 0., 1.];
            p.set_mod_hud(Some(&hud)).unwrap();
            frame(&mut p, &UiRuntime::new(1), [1280, 720], 1.);
            let nodes = &p.last_frame.as_ref().unwrap().nodes;
            let world_bounds = |node: &ui::UiNode| {
                let bounds = node.bounds();
                let mut offset = [0.; 2];
                let mut parent = node.parent();
                while let Some(id) = parent {
                    let ancestor = nodes.iter().find(|node| node.id() == id).unwrap();
                    offset[0] += ancestor.bounds().min().x();
                    offset[1] += ancestor.bounds().min().y();
                    parent = ancestor.parent();
                }
                ui::UiRect::new(
                    ui::UiPoint::new(bounds.min().x() + offset[0], bounds.min().y() + offset[1])
                        .unwrap(),
                    ui::UiPoint::new(bounds.max().x() + offset[0], bounds.max().y() + offset[1])
                        .unwrap(),
                )
                .unwrap()
            };
            let mesh_bounds = |color| {
                nodes
                    .iter()
                    .find_map(|node| {
                        let ui::UiVisual::Mesh(mesh) = node.visual() else {
                            return None;
                        };
                        mesh.vertices()
                            .iter()
                            .any(|vertex| vertex.color == color)
                            .then(|| world_bounds(node))
                    })
                    .expect("rendered row surface")
            };
            let surface = mesh_bounds([11, 13, 17, 209]);
            let progress = mesh_bounds([255, 0, 0, 255]);
            let unit = (surface.max().x() - surface.min().x()) / hud.cards[0].width;
            let row_end = surface.min().y() + height as f32 * unit;
            assert!(progress.max().y() <= row_end - 2. * unit + 0.01);
            assert!((progress.max().y() - progress.min().y() - 2. * unit).abs() < 0.01);
            let text: Vec<_> = nodes.iter().filter_map(|node| {
                matches!(node.visual(), ui::UiVisual::Text {color,..} if *color == [255;4] || *color == [255,0,0,255])
                    .then(|| world_bounds(node))
                    .filter(|bounds| bounds.min().y() >= surface.min().y() && bounds.max().y() <= surface.max().y())
            }).collect();
            assert_eq!(text.len(), 2);
            let legacy = layout == ui::mod_hud::RowLayout::Standard && height == 20;
            let gap = if legacy { 0. } else { 2. * unit };
            for bounds in text {
                assert!(bounds.min().y() >= surface.min().y() + 2. * unit - 0.01);
                assert!(
                    bounds.max().y() <= progress.min().y() - gap + 0.01,
                    "layout={layout:?}, row_height={height}, text={bounds:?}, progress={progress:?}"
                );
                if legacy {
                    assert!((bounds.min().y() - surface.min().y() - 2. * unit).abs() < 0.01);
                }
            }
            if legacy {
                assert!((progress.min().y() - surface.min().y() - 14. * unit).abs() < 0.01);
                let authored = template::card(&hud.cards[0], 0, &mut 0);
                let label = authored["controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find_map(|control| control.get("label_0"))
                    .unwrap();
                assert_eq!(label["offset"][1], 2.);
                assert_eq!(label["size"][1], 12.);
                assert_eq!(label["font_scale_factor"], 0.8);
            }
        }
    }
}
#[test]
fn cards_cannot_bypass_inventory_chat_loading_server_or_player_hud_visibility() {
    for gate in 0..5 {
        let mut p = presentation(gate == 0);
        let mut runtime = UiRuntime::new(1);
        match gate {
            1 => runtime.inventory_open = true,
            2 => runtime.chat_focused = true,
            3 => p.loading_stage = Some(super::super::LoadingStage::Connecting),
            4 => {
                let mut options = launcher::menu::settings_options::SettingsOptions::default();
                let index = launcher::menu::settings_options::SETTINGS_OPTIONS
                    .iter()
                    .position(|option| option.name == "hide_hud")
                    .unwrap();
                options.set(index, 1);
                p.set_chat_settings_snapshot((Arc::new(options), None));
            }
            _ => {}
        }
        let before = frame(&mut p, &runtime, [1280, 720], 1.);
        p.set_mod_hud(Some(&content())).unwrap();
        let gated = frame(&mut p, &runtime, [1280, 720], 1.);
        assert!(
            snapshot::rasterize(&before) == snapshot::rasterize(&gated),
            "gate {gate} hides the personal cards"
        );
    }
}
#[test]
fn anchored_card_bounds_and_icons_scale_together_across_viewports() {
    for (size, dpi, scale) in [
        ([1280, 720], 1., 0.5),
        ([1920, 1080], 1.5, 1.),
        ([2560, 1440], 2., 2.),
    ] {
        for anchor in [
            Anchor::TopLeft,
            Anchor::TopRight,
            Anchor::BottomLeft,
            Anchor::BottomRight,
        ] {
            let mut p = presentation(false);
            let mut hud = content();
            hud.cards[0].anchor = anchor;
            hud.cards[0].scale = scale;
            hud.cards[0].offset = match anchor {
                Anchor::TopLeft => [6., 6.],
                Anchor::TopRight => [-6., 6.],
                Anchor::BottomLeft => [6., -6.],
                Anchor::BottomRight => [-6., -6.],
            };
            p.set_mod_hud(Some(&hud)).unwrap();
            frame(&mut p, &UiRuntime::new(1), size, dpi);
            let nodes = &p.last_frame.as_ref().unwrap().nodes;
            let card = nodes
                .iter()
                .find(|node| matches!(node.visual(), ui::UiVisual::Mesh(_)))
                .expect("card surface");
            let rect = card.bounds();
            assert!(rect.min().x() >= 0. && rect.min().y() >= 0.);
            assert!(
                rect.max().x() <= size[0] as f32 / dpi && rect.max().y() <= size[1] as f32 / dpi
            );
        }
    }
}
#[test]
fn malformed_presentation_does_not_replace_the_existing_cards_or_crosshair() {
    let mut p = presentation(false);
    let hud = content();
    p.set_mod_hud(Some(&hud)).unwrap();
    let mut invalid = hud.clone();
    invalid.cards[0].scale = f32::NAN;
    assert!(p.set_mod_hud(Some(&invalid)).is_err());
    assert_eq!(
        p.form_presentation.mod_widgets.as_ref().unwrap().content,
        hud
    );
    let cursor = Crosshair::default();
    p.set_mod_crosshair(Some(&cursor)).unwrap();
    let invalid = Crosshair {
        gap: -1.,
        ..Default::default()
    };
    assert!(p.set_mod_crosshair(Some(&invalid)).is_err());
    assert_eq!(p.form_presentation.mod_crosshair, Some(cursor));
    p.set_mod_crosshair(None).unwrap();
    assert!(p.form_presentation.mod_crosshair.is_none());
}

#[test]
fn personal_hud_snapshot_with_real_carrier() {
    let Some(mut p) = installed_hud_presentation() else {
        eprintln!(
            "skipping personal_hud_snapshot_with_real_carrier: installed local carriers unavailable (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    p.hud_frame_mut().first_person = true;
    let before = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&before, "personal-hud-before");
    let equipment = Card {
        id: "equipment".into(),
        title: "Equipment".into(),
        anchor: Anchor::TopLeft,
        offset: [8., 24.],
        scale: 1.,
        rows: [
            ("Helmet", "minecraft:diamond_helmet", "65 / 363", 0.18),
            (
                "Chestplate",
                "minecraft:diamond_chestplate",
                "401 / 528",
                0.76,
            ),
            ("Leggings", "minecraft:diamond_leggings", "422 / 495", 0.85),
            ("Boots", "minecraft:diamond_boots", "429 / 429", 1.),
        ]
        .into_iter()
        .map(|(label, item, value, progress)| Row {
            label: label.into(),
            value: value.into(),
            item: Some(item.into()),
            effect_id: None,
            metadata: 0,
            progress: Some(progress),
            color: if progress < 0.2 {
                [1., 0.35, 0.3, 1.]
            } else {
                [0.3, 0.9, 0.5, 1.]
            },
        })
        .collect(),
        ..Default::default()
    };
    let effects = Card {
        id: "effects".into(),
        title: "Effects".into(),
        anchor: Anchor::TopRight,
        offset: [-8., 24.],
        scale: 1.,
        rows: [("Speed II", 1, "1:42"), ("Fire Resistance", 12, "6:15")]
            .into_iter()
            .map(|(label, id, value)| Row {
                label: label.into(),
                value: value.into(),
                item: None,
                effect_id: Some(id),
                metadata: 0,
                progress: None,
                color: [1.; 4],
            })
            .collect(),
        ..Default::default()
    };
    let supplies = Card {
        id: "supplies".into(),
        title: "Supplies".into(),
        anchor: Anchor::BottomLeft,
        offset: [8., -8.],
        scale: 1.,
        rows: [
            ("Healing pots", "minecraft:splash_potion", 22, "12"),
            ("Pearls", "minecraft:ender_pearl", 0, "16"),
            ("Arrows", "minecraft:arrow", 0, "64"),
            ("Blocks", "minecraft:stone", 0, "192"),
        ]
        .into_iter()
        .map(|(label, item, metadata, value)| Row {
            label: label.into(),
            value: value.into(),
            item: Some(item.into()),
            effect_id: None,
            metadata,
            progress: None,
            color: [1.; 4],
        })
        .collect(),
        ..Default::default()
    };
    let hud = Hud {
        cards: vec![equipment, effects, supplies],
        ..Default::default()
    };
    p.set_mod_hud(Some(&hud)).unwrap();
    for shape in [
        ui::mod_hud::CrosshairShape::Cross,
        ui::mod_hud::CrosshairShape::Dot,
        ui::mod_hud::CrosshairShape::Circle,
    ] {
        p.set_mod_crosshair(Some(&Crosshair {
            shape,
            color: [0.3, 1., 0.5, 1.],
            size: 4.,
            ..Default::default()
        }))
        .unwrap();
        let after = frame(&mut p, &runtime, [1920, 1080], 1.);
        snapshot::write(&after, &format!("personal-hud-after-{shape:?}"));
        assert!(
            snapshot::rasterize(&before) != snapshot::rasterize(&after),
            "personal HUD capture renders the enabled presentation"
        );
    }
    p.set_mod_hud(None).unwrap();
    p.set_mod_crosshair(None).unwrap();
    let restored = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&restored, "personal-hud-restored");
    assert!(
        snapshot::rasterize(&before) == snapshot::rasterize(&restored),
        "clearing personal HUD cards and cursor restores the original pixels"
    );
}

#[test]
fn custom_cursor_preserves_camera_spectator_hidden_hud_and_menu_gates() {
    use launcher::menu::settings_options::{
        SETTINGS_OPTIONS, SettingsOptions, THIRD_PERSON_CROSSHAIR_OPTION,
    };
    use protocol::PlayerGameMode;
    let Some(mut p) = installed_hud_presentation() else {
        eprintln!(
            "skipping custom_cursor_preserves_camera_spectator_hidden_hud_and_menu_gates: installed local carriers unavailable (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    p.set_mod_crosshair(Some(&Crosshair::default())).unwrap();
    for first_person in [false, true] {
        for third_person in [false, true] {
            for mode in [
                PlayerGameMode::Survival,
                PlayerGameMode::Creative,
                PlayerGameMode::Spectator,
            ] {
                for hidden in [false, true] {
                    let mut options = SettingsOptions::default();
                    for (name, value) in [
                        (THIRD_PERSON_CROSSHAIR_OPTION.name, third_person),
                        ("hide_hud", hidden),
                    ] {
                        let index = SETTINGS_OPTIONS
                            .iter()
                            .position(|o| o.name == name)
                            .unwrap();
                        options.set(index, i32::from(value));
                    }
                    player.facts.publish_player_game_mode(mode);
                    p.hud_frame_mut().first_person = first_person;
                    p.set_chat_settings_snapshot((Arc::new(options), None));
                    p.build(
                        &player,
                        &runtime,
                        0,
                        [1280, 720],
                        DpiScale::new(1.).unwrap(),
                    )
                    .unwrap();
                    let custom = p.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
                        let bounds = node.bounds();
                        matches!(node.visual(), ui::UiVisual::Mesh(_))
                            && ((bounds.min().x() + bounds.max().x()) * 0.5 - 640.).abs() < 0.01
                            && ((bounds.min().y() + bounds.max().y()) * 0.5 - 360.).abs() < 0.01
                    });
                    assert_eq!(
                        custom,
                        (first_person || third_person)
                            && mode != PlayerGameMode::Spectator
                            && !hidden,
                        "first person={first_person} third person={third_person} mode={mode:?} hidden={hidden}"
                    );
                }
            }
        }
    }
    p.set_menu_view(Some(launcher::menu::MenuView::new(true, "Fixture".into())));
    assert!(!p.mod_hud_visible(&player, &runtime));
}

#[test]
fn progress_fill_stays_within_its_track_for_icon_rows_and_each_card_scale() {
    fn mesh_bounds(presentation: &UiPresentationRuntime, color: [u8; 4]) -> Option<ui::UiRect> {
        presentation
            .last_frame
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|node| {
                let ui::UiVisual::Mesh(mesh) = node.visual() else {
                    return None;
                };
                mesh.vertices()
                    .iter()
                    .any(|vertex| vertex.color == color)
                    .then(|| node.bounds())
            })
    }
    for scale in [0.5, 1., 2.] {
        for icon in [false, true] {
            let mut presentation = presentation(false);
            let mut hud = content();
            hud.cards[0].scale = scale;
            if !icon {
                hud.cards[0].rows[0].item = None;
            }
            hud.cards[0].rows[0].color = [1., 0., 0., 1.];
            for progress in [0., 0.5, 1.] {
                hud.cards[0].rows[0].progress = Some(progress);
                presentation.set_mod_hud(Some(&hud)).unwrap();
                frame(&mut presentation, &UiRuntime::new(1), [1280, 720], 1.);
                let track =
                    mesh_bounds(&presentation, [255, 255, 255, 46]).expect("progress track");
                let fill = mesh_bounds(&presentation, [255, 0, 0, 255]);
                if progress == 0. {
                    assert!(fill.is_none(), "zero progress draws no fill");
                    continue;
                }
                let fill = fill.expect("positive progress fill");
                let width = |bounds: ui::UiRect| bounds.max().x() - bounds.min().x();
                assert_eq!(fill.min(), track.min());
                assert!(fill.max().x() <= track.max().x());
                assert!(
                    (width(fill) - width(track) * progress).abs() < 0.01,
                    "scale={scale}, icon={icon}, progress={progress}, fill={fill:?}, track={track:?}"
                );
            }
        }
    }
}

#[test]
fn rectangular_cells_center_text_resize_and_update_without_rebuilding() {
    use ui::mod_hud::Cell;
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let mut hud = Hud {
        cards: vec![Card {
            id: "buttons".into(),
            width: 56.,
            cells: vec![
                Cell {
                    rect: [19., 0., 18., 18.],
                    label: "W".into(),
                    background: [0., 0., 0., 0.44],
                    ..Default::default()
                },
                Cell {
                    rect: [0., 19., 27.5, 18.],
                    label: "LMB".into(),
                    value: "0 CPS".into(),
                    background: [0., 0., 0., 0.44],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    p.set_mod_hud(Some(&hud)).unwrap();
    let before = frame(&mut p, &runtime, [1280, 720], 1.);
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    hud.cards[0].cells[0].background = [1., 1., 1., 0.44];
    hud.cards[0].cells[0].color = [0., 0., 0., 1.];
    hud.cards[0].cells[1].value = "9 CPS".into();
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(Arc::ptr_eq(
        &catalog,
        &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
    ));
    let after = frame(&mut p, &runtime, [1280, 720], 1.);
    assert_ne!(snapshot::rasterize(&before), snapshot::rasterize(&after));
    assert_eq!(template::dimensions(&hud.cards[0]), [56., 37.]);
    hud.cards[0].scale = 2.;
    p.set_mod_hud(Some(&hud)).unwrap();
    assert_eq!(template::dimensions(&hud.cards[0]), [112., 74.]);
    let scaled = frame(&mut p, &runtime, [1280, 720], 1.);
    assert_ne!(snapshot::rasterize(&after), snapshot::rasterize(&scaled));
}
