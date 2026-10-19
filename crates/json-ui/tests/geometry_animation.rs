use std::sync::Arc;

use json_ui::{
    Animator, Catalog, Context, DrawNode, FormRender, GeometryLayout, HitRegion, LayoutEnv,
    TextMeasure, TextureMeta, TextureSource, ViewState,
};
use serde_json::{Value, json};

struct Metrics;
impl TextMeasure for Metrics {
    /// Gives original labels deterministic dimensions without installed carriers.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}
impl TextureSource for Metrics {
    /// Uses a small synthetic texture so sprite tessellation requires no files.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta::plain([16.0, 16.0]))
    }
}
/// Borrows the deterministic measurement sources shared by the original fixtures.
fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    }
}

struct Screen {
    layout: Option<GeometryLayout>,
    bound: json_ui::ResolvedControl,
    animator: Animator,
}
impl Screen {
    /// Resolves a tiny original screen with optional client-owned paint transforms.
    fn new(mut document: Value) -> Self {
        document["namespace"] = json!("fixture");
        let mut catalog = Catalog::default().with_paint_transforms();
        catalog.overlay_text("ui/fixture.json", &document.to_string());
        let bound = json_ui::resolve(&catalog, "fixture.root", &Context::desktop())
            .control
            .unwrap();
        Self {
            layout: GeometryLayout::new(&bound, [200.0, 100.0], &env(), &ViewState::default()),
            bound,
            animator: Animator::new(),
        }
    }
    /// Samples eligible entrances, otherwise preserving the original actual-state layout.
    fn sample(
        &mut self,
        now: f64,
        clocks: Option<&std::collections::BTreeMap<String, f64>>,
        view: &ViewState,
    ) -> Arc<FormRender> {
        self.layout.as_mut().map_or_else(
            || {
                Arc::new(json_ui::render_bound(
                    self.bound.clone(),
                    [200.0, 100.0],
                    &env(),
                    view,
                ))
            },
            |layout| layout.sample(&mut self.animator, now, clocks, &env(), view),
        )
    }
    /// Runs the form's layout selection and ordinary paint lifecycle at supplied time.
    fn frame(&mut self, now: f64, view: &ViewState) -> Arc<FormRender> {
        let frame = self.sample(now, None, view);
        for node in frame.nodes.iter() {
            if node.shown(view) {
                node.animate(&mut self.animator, now, None, Some(&Metrics));
            }
        }
        self.animator.end_frame();
        frame
    }
}

#[test]
fn size_animation_resolves_own_axes_against_its_original_dimensions() {
    for to in [["200%x", "200%y"], ["400%y", "100%x"]] {
        let mut screen = Screen::new(json!({
            "grow":{"anim_type":"size","from":["100%x","100%y"],"to":to,"duration":1},
            "root":{"type":"button","size":[40,20],"anims":["@fixture.grow"],"anchor_from":"top_left","anchor_to":"top_left"}
        }));
        let initial = screen.frame(0.0, &ViewState::default());
        assert_eq!(
            [hit(&initial, "root").rect.w, hit(&initial, "root").rect.h],
            [40.0, 20.0]
        );
        let halfway = screen.frame(0.5, &ViewState::default());
        assert_eq!(
            [hit(&halfway, "root").rect.w, hit(&halfway, "root").rect.h],
            [60.0, 30.0]
        );
        let final_frame = screen.frame(1.0, &ViewState::default());
        assert_eq!(
            [
                hit(&final_frame, "root").rect.w,
                hit(&final_frame, "root").rect.h
            ],
            [80.0, 40.0]
        );
    }
}

#[test]
fn maximum_terms_interpolate_their_resolved_contributions() {
    for (from, to, midpoint, end) in [
        ("0px", "100%cm", 20.0, 40.0),
        ("0px", "-100%cm + 100%cm", 40.0, 80.0),
        ("100%cm + 100%cm", "0px", 40.0, 0.0),
        ("100%cm", "0%cm", 20.0, 0.0),
    ] {
        for channel in ["offset", "size"] {
            let mut screen = Screen::new(json!({
                "move":{"anim_type":channel,"from":[from,10],"to":[to,10],"duration":1},
                "root":{"type":"button","size":[100,20],"anims":["@fixture.move"],"anchor_from":"top_left","anchor_to":"top_left","controls":[
                    {"child":{"type":"panel","size":[40,10]}}
                ]}
            }));
            screen.frame(0.0, &ViewState::default());
            for (now, expected) in [(0.5, midpoint), (1.0, end)] {
                let frame = screen.frame(now, &ViewState::default());
                let rect = hit(&frame, "root").rect;
                assert_eq!(
                    if channel == "size" { rect.w } else { rect.x },
                    expected,
                    "{channel}: {from} to {to}"
                );
            }
        }
    }
}

#[test]
fn interaction_hidden_geometry_retires_and_restarts_without_hidden_events() {
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[0,0],"to":[20,0],"duration":1,"end_event":"fixture.done"},
        "fade":{"anim_type":"alpha","from":0,"to":1,"duration":1,"end_event":"fixture.faded"},
        "root":{"type":"button","size":[100,50],"anchor_from":"top_left","anchor_to":"top_left","default_control":"idle","hover_control":"over","pressed_control":"pressed","controls":[
            {"idle":{"type":"image","size":[20,10],"color":[1,0,0],"anims":["@fixture.move","@fixture.fade"]}},
            {"over":{"type":"image","size":[20,10],"color":[0,1,0],"offset":"@fixture.move"}},
            {"pressed":{"type":"image","size":[20,10],"color":[0,0,1],"offset":"@fixture.move"}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let mut hidden = ViewState {
        hovered: Some("/root".into()),
        ..ViewState::default()
    };
    screen.frame(0.5, &hidden);
    assert!(screen.animator.take_events().is_empty());
    hidden.hovered = None;
    hidden.pressed = Some("/root".into());
    screen.frame(2.0, &hidden);
    assert!(
        screen.animator.take_events().is_empty(),
        "hidden state programs must not complete"
    );
    let back = screen.frame(3.0, &ViewState::default());
    let idle = draw(&back, "idle");
    let paint = idle.animate(&mut screen.animator, 3.0, None, Some(&Metrics));
    assert_eq!(paint.dest, idle.dest);
    assert_eq!(
        paint.opacity, 0.0,
        "hidden alpha channel must restart with geometry"
    );
    assert_eq!(
        idle.dest.x, 40.0,
        "centered idle returns to its initial offset"
    );
    assert!(screen.animator.take_events().is_empty());
}

#[test]
fn geometry_programs_in_initially_hidden_states_use_layout_when_shown() {
    let mut screen = Screen::new(json!({
        "grow":{"anim_type":"size","from":[0,0],"to":[40,20],"duration":1},
        "root":{"type":"button","size":[100,50],"anchor_from":"top_left","anchor_to":"top_left","default_control":"idle","hover_control":"over","controls":[
            {"idle":{"type":"image","size":[10,10],"color":[1,0,0]}},
            {"over":{"type":"image","size":"@fixture.grow","color":[0,1,0]}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let view = ViewState {
        hovered: Some("/root".into()),
        ..ViewState::default()
    };
    screen.frame(1.0, &view);
    let frame = screen.frame(2.0, &view);
    let rect = draw(&frame, "over").dest;
    assert_eq!([rect.x, rect.y, rect.w, rect.h], [30.0, 15.0, 40.0, 20.0]);
}

#[test]
fn recreated_waiting_instances_restore_both_present_and_absent_baselines() {
    for offset in [Some(json!([3, 4])), None] {
        let mut root = json!({"type":"button","size":[40,20],"anchor_from":"top_left","anchor_to":"top_left","anims":["@fixture.move","@fixture.grow"],"controls":[{"fill":{"type":"image","color":[1,0,0]}}]});
        if let Some(offset) = offset {
            root["offset"] = offset;
        }
        let mut screen = Screen::new(json!({
            "move":{"anim_type":"offset","from":[10,10],"to":[20,20],"duration":1,"play_event":"fixture.play"},
            "grow":{"anim_type":"size","from":[50,30],"to":[60,40],"duration":1,"play_event":"fixture.play"},
            "root":root
        }));
        let original = hit(&screen.frame(0.0, &ViewState::default()), "root").rect;
        screen.animator.fire("fixture.play");
        let played = screen.frame(1.0, &ViewState::default());
        assert!(
            screen.layout.is_none(),
            "event-driven multi-channel programs stay in paint"
        );
        assert_eq!(
            draw(&played, "fill")
                .animate(&mut screen.animator, 1.0, None, Some(&Metrics))
                .dest
                .w,
            60.0
        );
        screen.animator.end_frame();
        let reopened = screen.frame(2.0, &ViewState::default());
        assert_eq!(
            hit(&reopened, "root").rect,
            original,
            "waiting channels use original properties"
        );
    }
}
/// Returns a drawn primitive by its original fixture name.
fn draw<'a>(frame: &'a FormRender, name: &str) -> &'a DrawNode {
    frame
        .nodes
        .iter()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("missing draw {name}"))
}
/// Returns an interactive control by its original fixture name.
fn hit<'a>(frame: &'a FormRender, name: &str) -> &'a HitRegion {
    frame
        .hits
        .iter()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("missing hit {name}"))
}

#[test]
fn zero_sized_growth_uses_current_size_and_anchor() {
    for from in [[0, 0], [0, 10], [10, 0]] {
        for (anchor, expected) in [("center", [80.0, 40.0]), ("bottom_right", [160.0, 80.0])] {
            let mut screen = Screen::new(json!({
                "grow":{"anim_type":"size","from":from,"to":[40,20],"duration":1},
                "root":{"type":"image","texture":"textures/fixture","keep_ratio":false,"size":"@fixture.grow","anchor_from":anchor,"anchor_to":anchor}
            }));
            screen.frame(0.0, &ViewState::default());
            let frame = screen.frame(1.0, &ViewState::default());
            let rect = draw(&frame, "root").dest;
            assert_eq!(
                [rect.x, rect.y, rect.w, rect.h],
                [expected[0], expected[1], 40.0, 20.0]
            );
        }
    }
}

#[test]
fn destroyed_geometry_hides_static_descendants_across_frames() {
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[0,0],"to":[20,0],"duration":1,"destroy_at_end":"root"},
        "root":{"type":"panel","size":[100,50],"offset":"@fixture.move","controls":[
            {"fill":{"type":"image","size":[20,10],"color":[1,0,0]}}
        ]}
    }));
    let frame = screen.frame(0.0, &ViewState::default());
    let mut fill = draw(&frame, "fill").clone();
    let ancestor = Arc::clone(&fill.anim.as_ref().unwrap().movers[0]);
    fill.anim = None;
    for now in [0.0, 1.0, 2.0, 3.0] {
        if now <= 1.0 {
            screen.animator.sample(&ancestor, now, None);
        }
        let painted = fill.animate(&mut screen.animator, now, None, Some(&Metrics));
        assert_eq!(painted.hidden, now >= 1.0);
        screen.animator.end_frame();
        if now >= 1.0 {
            assert!(screen.animator.is_destroyed("/root/fill"));
        }
    }
}

#[test]
fn parent_growth_reflows_fixed_and_percentage_children() {
    let mut screen = Screen::new(json!({
        "grow":{"anim_type":"size","from":[0,0],"to":[200,100],"duration":1},
        "root":{"type":"panel","size":"@fixture.grow","anchor_from":"top_left","anchor_to":"top_left","controls":[
            {"fixed":{"type":"button","size":[20,10]}},
            {"relative":{"type":"image","texture":"textures/fixture","keep_ratio":false,"size":["50%","50%"]}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let frame = screen.frame(1.0, &ViewState::default());
    let fixed = hit(&frame, "fixed").rect;
    assert_eq!(
        [fixed.x, fixed.y, fixed.w, fixed.h],
        [90.0, 45.0, 20.0, 10.0]
    );
    let relative = draw(&frame, "relative").dest;
    assert_eq!(
        [relative.x, relative.y, relative.w, relative.h],
        [50.0, 25.0, 100.0, 50.0]
    );
}

#[test]
fn mixed_units_follow_a_parent_resizing_at_the_same_clock() {
    let mut screen = Screen::new(json!({
        "grow":{"anim_type":"size","from":[100,50],"to":[200,100],"duration":1},
        "move":{"anim_type":"offset","from":[10,0],"to":["50%","-100%"],"duration":1},
        "root":{"type":"panel","size":"@fixture.grow","anchor_from":"top_left","anchor_to":"top_left","controls":[
            {"press":{"type":"button","size":[10,10],"offset":"@fixture.move","anchor_from":"top_left","anchor_to":"top_left"}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let frame = screen.frame(0.5, &ViewState::default());
    let rect = hit(&frame, "press").rect;
    assert_eq!([rect.x, rect.y, rect.w, rect.h], [42.5, -37.5, 10.0, 10.0]);
}

#[test]
fn offset_expressions_keep_live_own_and_child_sum_dimensions() {
    for unit in ["own", "children"] {
        let mut screen = Screen::new(if unit == "own" {
            json!({
                "grow":{"anim_type":"size","from":[40,20],"to":[80,40],"duration":1},
                "move":{"anim_type":"offset","from":[0,0],"to":["100%x","100%y"],"duration":1},
                "root":{"type":"button","size":[40,20],"anchor_from":"top_left","anchor_to":"top_left","anims":["@fixture.grow","@fixture.move"],"controls":[{"fill":{"type":"image","color":[1,0,0]}}]}
            })
        } else {
            json!({
                "grow":{"anim_type":"size","from":[40,20],"to":[80,40],"duration":1},
                "move":{"anim_type":"offset","from":[0,0],"to":["100%c","100%c"],"duration":1},
                "root":{"type":"button","size":[100,50],"offset":"@fixture.move","anchor_from":"top_left","anchor_to":"top_left","controls":[
                    {"child":{"type":"panel","size":[40,20],"anims":["@fixture.grow"]}}
                ]}
            })
        });
        screen.frame(0.0, &ViewState::default());
        let frame = screen.frame(0.5, &ViewState::default());
        if unit == "own" {
            assert!(
                screen.layout.is_none(),
                "two geometry properties keep legacy composition"
            );
            let rect = draw(&frame, "fill")
                .animate(&mut screen.animator, 0.5, None, Some(&Metrics))
                .dest;
            assert_eq!([rect.x, rect.y], [20.0, 10.0]);
            assert_eq!(hit(&frame, "root").rect.x, 0.0, "legacy input is unchanged");
        } else {
            let rect = hit(&frame, "root").rect;
            assert_eq!(
                [rect.x, rect.y],
                [30.0, 15.0],
                "child-sum dimensions remain live"
            );
        }
    }
}

#[test]
fn geometry_resets_write_from_and_changed_birth_clocks_restore_waiting_baselines() {
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[10,10],"to":[30,30],"duration":1,"play_event":"fixture.play","reset_event":"fixture.reset"},
        "grow":{"anim_type":"size","from":[50,30],"to":[70,50],"duration":1,"play_event":"fixture.play","reset_event":"fixture.reset"},
        "root":{"type":"button","size":[40,20],"offset":[3,4],"anims":["@fixture.move","@fixture.grow"],"anim_clock":"fixture.birth","anchor_from":"top_left","anchor_to":"top_left","controls":[{"fill":{"type":"image","color":[1,0,0]}}]}
    }));
    screen.frame(0.0, &ViewState::default());
    screen.animator.fire("fixture.play");
    let frame = screen.frame(0.5, &ViewState::default());
    assert!(screen.layout.is_none());
    let rect = draw(&frame, "fill")
        .animate(&mut screen.animator, 0.5, None, Some(&Metrics))
        .dest;
    assert_eq!([rect.x, rect.y, rect.w, rect.h], [20.0, 20.0, 60.0, 40.0]);
    screen.animator.fire("fixture.reset");
    let reset = screen.frame(0.75, &ViewState::default());
    let rect = draw(&reset, "fill")
        .animate(&mut screen.animator, 0.75, None, Some(&Metrics))
        .dest;
    assert_eq!([rect.x, rect.y, rect.w, rect.h], [10.0, 10.0, 50.0, 30.0]);
    let clocks = std::collections::BTreeMap::from([("fixture.birth".into(), 1.0)]);
    let recreated = screen.sample(1.0, Some(&clocks), &ViewState::default());
    let rect = draw(&recreated, "fill")
        .animate(&mut screen.animator, 1.0, Some(&clocks), Some(&Metrics))
        .dest;
    assert_eq!([rect.x, rect.y, rect.w, rect.h], [3.0, 4.0, 40.0, 20.0]);
}

#[test]
fn entering_page_rebuilds_nested_clips_and_input_geometry() {
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[0,100],"to":[0,0],"duration":1},
        "root":{"type":"panel","size":[200,100],"clips_children":true,"clip_offset":[10,10],"controls":[
            {"page":{"type":"panel","size":[200,100],"offset":"@fixture.move","anchor_from":"top_left","anchor_to":"top_left","clips_children":true,"clip_offset":[20,5],"controls":[
                {"middle":{"type":"panel","controls":[
                    {"press":{"type":"button","size":["100%","100%"],"button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"fixture.choose","mapping_type":"pressed"}],"controls":[
                        {"fill":{"type":"image","texture":"textures/fixture","keep_ratio":false,"size":["100%","100%"]}}
                    ]}}
                ]}}
            ]}}
        ]}
    }));
    let initial = screen.frame(0.0, &ViewState::default());
    assert!(initial.nodes.is_empty());
    let middle = screen.frame(0.5, &ViewState::default());
    assert_eq!(draw(&middle, "fill").clip.h, 35.0);
    let frame = screen.frame(1.0, &ViewState::default());
    let fill = draw(&frame, "fill");
    assert_eq!(
        [fill.clip.x, fill.clip.y, fill.clip.w, fill.clip.h],
        [20.0, 10.0, 160.0, 80.0]
    );
    assert_eq!(hit(&frame, "press").clip, fill.clip);
    assert!(initial.nodes.is_empty(), "earlier snapshots stay immutable");
    assert_eq!(draw(&middle, "fill").clip.h, 35.0);
    assert!(json_ui::hit_test(&frame.hits, [30.0, 20.0]).is_some());
    assert!(json_ui::hit_test(&frame.hits, [30.0, 120.0]).is_none());
    assert!(json_ui::hit_test(&frame.hits, [15.0, 20.0]).is_none());
}

#[test]
fn paint_transform_applies_once_after_sampled_geometry_and_input() {
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[0,0],"to":[20,10],"duration":1},
        "root":{"type":"panel","size":[40,20],"offset":"@fixture.move","anchor_from":"top_left","anchor_to":"top_left","cinnabar_transform":{"scale":2,"origin":[0,0]},"controls":[
            {"press":{"type":"button","size":[20,10],"anchor_from":"top_left","anchor_to":"top_left","controls":[
                {"fill":{"type":"image","texture":"textures/fixture","keep_ratio":false}}
            ]}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let frame = screen.frame(1.0, &ViewState::default());
    let fill = draw(&frame, "fill");
    assert_eq!([fill.dest.x, fill.dest.y], [20.0, 10.0]);
    assert_eq!(
        fill.anim.as_ref().unwrap().transform,
        hit(&frame, "press").transform
    );
    assert!(hit(&frame, "press").contains([55.0, 25.0]));
    assert!(!hit(&frame, "press").contains([65.0, 25.0]));
}

#[test]
fn geometry_refresh_preserves_scroll_culling_and_settled_frame_reuse() {
    let rows:Vec<_>=(0..20).map(|index|json!({format!("row{index}"):{"type":"button","size":[100,10],"controls":[{"label":{"type":"label","text":format!("Row {index}"),"size":[100,10]}}]}})).collect();
    let mut screen = Screen::new(json!({
        "move":{"anim_type":"offset","from":[0,100],"to":[0,0],"duration":1},
        "root":{"type":"scroll_view","size":[100,50],"offset":"@fixture.move","anchor_from":"top_left","anchor_to":"top_left","scroll_view_port":"port","scroll_content":"content","scrollbar_track":"track","scrollbar_box":"box","controls":[
            {"port":{"type":"panel","size":[100,50],"clips_children":true,"anchor_from":"top_left","anchor_to":"top_left","controls":[
                {"content":{"type":"stack_panel","size":[100,"100%c"],"anchor_from":"top_left","anchor_to":"top_left","controls":rows}}
            ]}},
            {"track":{"type":"scroll_track","size":[4,50],"anchor_from":"top_right","anchor_to":"top_right","controls":[
                {"box":{"type":"scrollbar_box","size":[4,50],"draggable":"vertical"}}
            ]}}
        ]}
    }));
    screen.frame(0.0, &ViewState::default());
    let arrived = screen.frame(1.0, &ViewState::default());
    assert!(screen.animator.take_events().is_empty());
    assert!(json_ui::hit_test(&arrived.hits, [5.0, 5.0]).is_some());
    assert!(arrived.nodes.len() < 20, "offscreen rows stay culled");
    let mut view = ViewState::default();
    view.scroll.insert("/root".into(), 100.0);
    let scrolled = screen.frame(1.5, &view);
    assert!(!hit(&scrolled, "row0").contains([5.0, 5.0]));
    assert!(hit(&scrolled, "row10").contains([5.0, 5.0]));
    assert!(
        !scrolled
            .nodes
            .iter()
            .any(|node| matches!(&node.draw, json_ui::Draw::Text { text, .. } if text == "Row 0"))
    );
    let settled = screen.frame(2.0, &view);
    assert!(
        Arc::ptr_eq(&scrolled, &settled),
        "unchanged geometry reuses its frame"
    );
    assert!(
        screen.animator.take_events().is_empty(),
        "re-layout must not restart animation events"
    );
}
