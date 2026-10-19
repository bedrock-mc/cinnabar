use super::*;

/// Compares public form paint and input with the ordinary cached-screen path at three times.
fn legacy_positions(root: Value, animations: Value, view: ViewState) -> Vec<[f32; 2]> {
    let catalog = catalog(root, animations);
    let mut form = engine(catalog.clone());
    let mut baseline = engine(catalog);
    let catalog = Arc::clone(baseline.catalog());
    let mut cached = CachedScreen::default();
    let mut positions = Vec::new();
    for now in [0.0, 0.5, 1.0] {
        let (frame, actual) = paint(&mut form, now, |engine, frame| {
            engine.render(&model(), &view, frame)
        });
        let (expected_frame, expected) = paint(&mut baseline, now, |engine, frame| {
            let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
            engine.draw(
                ScreenArt {
                    view: Some(&view),
                    ..ScreenArt::default()
                },
                frame,
                |env, root| {
                    cached.render_with(
                        "fixture.root",
                        &catalog,
                        &Context::desktop(),
                        DataSource::new(),
                        (root, px, [0; 3]),
                        env,
                        &view,
                    )
                },
            )
        });
        let solids = |nodes: &[UiNode]| {
            nodes
                .iter()
                .filter(|node| matches!(node.visual(), UiVisual::Solid { .. }))
                .map(UiNode::bounds)
                .collect::<Vec<_>>()
        };
        let actual = solids(&actual);
        let expected = solids(&expected);
        assert!(!expected.is_empty(), "the baseline must paint at {now}");
        assert_eq!(
            actual, expected,
            "replacement placement must retain paint movement at {now}"
        );
        let hit = frame.hits.iter().find(|hit| hit.name == "press").unwrap();
        let expected_hit = expected_frame
            .hits
            .iter()
            .find(|hit| hit.name == "press")
            .unwrap();
        assert_eq!(
            hit.rect, expected_hit.rect,
            "legacy input placement at {now}"
        );
        positions.push([actual[0].min().x(), actual[0].min().y()]);
    }
    positions
}

/// Builds an original offset animation and a button whose fill follows its owner.
fn offset_fixture(mut moving: Value) -> (Value, Value) {
    moving.as_object_mut().unwrap().extend(json!({
        "type":"panel","size":[40,20],"offset":"@fixture.move",
        "controls":[{"press":{"type":"button","controls":[{"fill":{"type":"image","color":[1,0,0]}}]}}]
    }).as_object().unwrap().clone());
    (
        json!({"type":"panel","size":[200,100],"controls":[{"moving":moving}]}),
        json!({"move":{"anim_type":"offset","from":[0,0],"to":[20,10],"duration":1}}),
    )
}

#[test]
fn anchored_offset_replacement_keeps_ordinary_paint_movement() {
    for anchor in ["top_left", "bottom_right", "top_middle"] {
        let (root, animations) = offset_fixture(json!({
            "anchor_from":anchor,"anchor_to":"top_left","use_anchored_offset":true,
            "property_bag":{"#anchored_offset_value_x":0.5,"#anchored_offset_value_y":0.25}
        }));
        let positions = legacy_positions(root, animations, ViewState::default());
        assert_eq!(positions[2][0] - positions[0][0], 20.0);
        assert_eq!(positions[2][1] - positions[0][1], 10.0);
    }
}

#[test]
fn anchored_offset_ancestor_keeps_pure_child_on_ordinary_paint_path() {
    let (mut root, mut animations) = offset_fixture(json!({
        "anchor_from":"top_left","anchor_to":"top_left","use_anchored_offset":true,
        "property_bag":{"#anchored_offset_value_x":0.5}
    }));
    root["controls"][0]["moving"]["controls"][0]["press"]["size"] = json!("@fixture.grow");
    root["offset"] = json!("@fixture.hold");
    animations["hold"] = json!({"anim_type":"offset","from":[0,0],"to":[0,0],"duration":2});
    animations["grow"] = json!({"anim_type":"size","from":[10,10],"to":[20,20],"duration":1});
    let positions = legacy_positions(root, animations, ViewState::default());
    assert_eq!(positions.first(), Some(&[15.0, 5.0]));
    assert_eq!(positions.last(), Some(&[30.0, 10.0]));
}

#[test]
fn zero_and_centered_anchored_modes_keep_layout_and_input() {
    for (anchor, fraction, base) in [
        ("top_left", 0.0, [0.0, 0.0]),
        ("center", 0.5, [100.0, 50.0]),
    ] {
        let (root, animations) = offset_fixture(json!({
            "anchor_from":anchor,"anchor_to":"top_left","use_anchored_offset":true,
            "property_bag":{"#anchored_offset_value_x":fraction,"#anchored_offset_value_y":fraction}
        }));
        let mut form = engine(catalog(root, animations));
        for now in [0.0, 0.5, 1.0] {
            let (frame, nodes) = paint(&mut form, now, |engine, frame| {
                engine.render(&model(), &ViewState::default(), frame)
            });
            let hit = frame.hits.iter().find(|hit| hit.name == "press").unwrap();
            let expected = [base[0] + 20.0 * now, base[1] + 10.0 * now];
            assert_eq!([hit.rect.x, hit.rect.y], expected);
            assert!(hit.contains([expected[0] + 5.0, expected[1] + 5.0]));
            let solid = nodes
                .iter()
                .find(|node| matches!(node.visual(), UiVisual::Solid { .. }))
                .unwrap();
            assert_eq!(
                [solid.bounds().min().x(), solid.bounds().min().y()],
                expected.map(|axis| axis as f32)
            );
        }
    }
}

#[test]
fn pointer_owned_placement_keeps_ordinary_paint_movement() {
    for flag in ["follows_cursor", "follows_cursor_inside_parent"] {
        let (root, animations) = offset_fixture(json!({
            "anchor_from":"top_left","anchor_to":"top_left",flag:true
        }));
        let positions = legacy_positions(
            root,
            animations,
            ViewState {
                pointer: Some([80.0, 40.0]),
                ..ViewState::default()
            },
        );
        assert_eq!(positions[2][0] - positions[0][0], 20.0);
        assert_eq!(positions[2][1] - positions[0][1], 10.0);
    }
}

#[test]
fn widget_owned_placement_keeps_ordinary_paint_movement() {
    let (mut slider, animations) = offset_fixture(json!({
        "anchor_from":"top_left","anchor_to":"top_left"
    }));
    slider["type"] = json!("slider");
    slider["#slider_value"] = json!(0.5);
    slider["slider_box_control"] = json!("moving");

    let (mut area, _) = offset_fixture(json!({
        "anchor_from":"top_left","anchor_to":"top_left"
    }));
    area["controls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"drop":{
            "type":"dropdown","size":[40,20],"offset":[0,50],
            "anchor_from":"top_left","anchor_to":"top_left",
            "dropdown_area":"area","dropdown_content_control":"moving"
        }}));
    let dropdown = json!({"type":"panel","size":[200,100],"controls":[{"area":area}]});

    let (root, _) = offset_fixture(json!({
        "anchor_from":"top_left","anchor_to":"top_left","draggable":"vertical"
    }));
    let moving = root["controls"][0]["moving"].clone();
    let scroll = json!({"type":"scroll_view","size":[200,100],
        "scroll_view_port":"port","scroll_content":"content","scrollbar_track":"track","scrollbar_box":"moving",
        "controls":[
            {"port":{"type":"panel","size":[200,100],"controls":[{"content":{"type":"panel","size":[200,200]}}]}},
            {"track":{"type":"panel","size":[40,100],"controls":[{"moving":moving}]}}
        ]
    });
    for root in [slider, dropdown, scroll] {
        let positions = legacy_positions(root, animations.clone(), ViewState::default());
        assert_eq!(positions[2][0] - positions[0][0], 20.0);
        assert_eq!(positions[2][1] - positions[0][1], 10.0);
    }
}

#[test]
fn packed_offset_animations_remain_inert() {
    for kind in ["stack_panel", "grid"] {
        let (mut root, animations) = offset_fixture(json!({
            "anchor_from":"top_left","anchor_to":"top_left","grid_position":[0,0]
        }));
        root["type"] = json!(kind);
        root["grid_dimensions"] = json!([1, 1]);
        let positions = legacy_positions(root, animations, ViewState::default());
        assert_eq!(positions, [[0.0, 0.0]; 3]);
    }
}

#[test]
fn ordinary_offset_entrances_keep_drag_addition_and_containment() {
    for drag in [5.0, 150.0] {
        let (root, animations) = offset_fixture(json!({
            "anchor_from":"top_left","anchor_to":"top_left","draggable":"horizontal","contained":true
        }));
        let mut form = engine(catalog(root, animations));
        let name = json_ui::form_template(&model()).split_once('.').unwrap().1;
        let mut view = ViewState::default();
        view.drags.insert(format!("/{name}/moving"), [drag, 0.0]);
        for now in [0.0, 0.5, 1.0] {
            let (frame, nodes) = paint(&mut form, now, |engine, frame| {
                engine.render(&model(), &view, frame)
            });
            let hit = frame.hits.iter().find(|hit| hit.name == "press").unwrap();
            assert_eq!(hit.rect.x, (drag + 20.0 * now).min(160.0));
            let solid = nodes
                .iter()
                .find(|node| matches!(node.visual(), UiVisual::Solid { .. }))
                .unwrap();
            assert_eq!(solid.bounds().min().x(), hit.rect.x as f32);
            assert!(hit.contains([hit.rect.x + 5.0, hit.rect.y + 5.0]));
        }
    }
}

#[test]
fn ordinary_size_entrances_keep_layout_bounds() {
    let (mut root, mut animations) = offset_fixture(json!({
        "anchor_from":"top_left","anchor_to":"top_left","min_size":[20,10],"max_size":[40,20]
    }));
    let moving = root["controls"][0]["moving"].as_object_mut().unwrap();
    moving.remove("offset");
    moving.insert("size".into(), json!("@fixture.grow"));
    animations["grow"] = json!({"anim_type":"size","from":[10,5],"to":[100,50],"duration":1});
    let mut form = engine(catalog(root, animations));
    for (now, expected) in [
        (0.0, [20.0, 10.0]),
        (0.5, [40.0, 20.0]),
        (1.0, [40.0, 20.0]),
    ] {
        let (frame, _) = paint(&mut form, now, |engine, frame| {
            engine.render(&model(), &ViewState::default(), frame)
        });
        let hit = frame.hits.iter().find(|hit| hit.name == "press").unwrap();
        assert_eq!([hit.rect.w, hit.rect.h], expected);
        assert!(hit.contains([expected[0] - 1.0, expected[1] - 1.0]));
        assert!(!hit.contains([expected[0] + 1.0, expected[1] + 1.0]));
    }
}
