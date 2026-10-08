//! Animation parity against the 1.26.50 `UIAnimationComponent`: every type,
//! chains, events, lifecycle flags, flip-book geometry and property writes.

use json_ui::{
    AnimEvent, Animated, Animator, AsepriteFrame, Catalog, Context, Draw, DrawNode, LayoutEnv,
    TextMeasure, TextureMeta, TextureSource, emit, layout, resolve,
};

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

/// `textures/ui/strip` is 32x8 (four 8x8 frames); `textures/ui/sheet` is aseprite.
struct Strip;
impl TextureSource for Strip {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        let base_size = match path {
            "textures/ui/strip" => [32.0, 8.0],
            "textures/ui/tall" => [8.0, 32.0],
            _ => [16.0, 16.0],
        };
        Some(TextureMeta::plain(base_size))
    }

    fn aseprite_frames(&self, path: &str) -> Option<std::sync::Arc<[AsepriteFrame]>> {
        (path == "textures/ui/sheet").then(|| {
            vec![
                AsepriteFrame {
                    x: 10,
                    y: 0,
                    duration_ms: 1000,
                },
                AsepriteFrame {
                    x: 84,
                    y: 0,
                    duration_ms: 100,
                },
            ]
            .into()
        })
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &Strip,
    }
}

/// The draws of `root` resolved from the `audit` namespace `body`.
fn draws(body: &str, root: &str) -> Vec<DrawNode> {
    let screen = format!(r#"{{ "namespace": "audit", {body} }}"#);
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/audit.json"]}"#.as_slice(),
        ),
        ("ui/audit.json", screen.as_bytes()),
    ])
    .expect("catalog");
    let control = resolve(&catalog, &format!("audit.{root}"), &Context::desktop())
        .control
        .expect("control");
    let env = env();
    emit(&layout(&control, [200.0, 200.0], &env), &env)
}

fn node<'a>(nodes: &'a [DrawNode], name: &str) -> &'a DrawNode {
    nodes
        .iter()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("no draw {name}"))
}

/// Paints `node` every frame of 1/60 s from 0 to `until`, returning the last draw.
fn play(animator: &mut Animator, node: &DrawNode, from: f64, until: f64) -> Animated {
    let frames = ((until - from) * 60.0).round() as usize;
    let mut drawn = node.animate(animator, from, None, Some(&Strip));
    for frame in 1..=frames {
        drawn = node.animate(animator, from + frame as f64 / 60.0, None, Some(&Strip));
    }
    drawn
}

fn close(value: f32, expected: f32) -> bool {
    (value - expected).abs() < 0.02
}

// N1: an animation writes alpha over a static zero; a starting-alpha scale multiplies it.
#[test]
fn alpha_animation_writes_the_property() {
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 0, "to": 1, "duration": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "size": [10, 10],
            "alpha": 0, "anims": ["@audit.fade"] }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.5);
    assert!(close(drawn.opacity, 0.5), "{}", drawn.opacity);
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 0, "to": 1, "duration": 1,
            "scale_from_starting_alpha": true },
        "image": { "type": "image", "texture": "textures/ui/test", "size": [10, 10],
            "alpha": 0.5, "anims": ["@audit.fade"] }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.5);
    assert!(close(drawn.opacity, 0.25), "{}", drawn.opacity);
}

// N40: two identical writers leave the last write, not a product.
#[test]
fn simultaneous_alpha_writers_overwrite() {
    let nodes = draws(
        r#""a": { "anim_type": "alpha", "from": 1, "to": 0.5, "duration": 1 },
        "b": { "anim_type": "alpha", "from": 1, "to": 0.5, "duration": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "size": [10, 10],
            "anims": ["@audit.a", "@audit.b"] }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.5);
    assert!(close(drawn.opacity, 0.75), "{}", drawn.opacity);
}

// N2-N6: offset, size, color, uv and clip animations reach the draw halfway through.
#[test]
fn every_property_type_animates() {
    let nodes = draws(
        r#""move": { "anim_type": "offset", "from": [0, 0], "to": [20, 0], "duration": 1 },
        "grow": { "anim_type": "size", "from": [10, 10], "to": [30, 20], "duration": 1 },
        "tint": { "anim_type": "color", "from": [0, 1, 0, 1], "to": [1, 0, 0, 1], "duration": 1 },
        "pan": { "anim_type": "uv", "from": [0, 0], "to": [16, 0], "duration": 1 },
        "reveal": { "anim_type": "clip", "from": 1, "to": 0, "duration": 1 },
        "image": { "type": "image", "texture": "textures/ui/strip", "size": [10, 10],
            "uv_size": [8, 8], "anchor_from": "top_left", "anchor_to": "top_left",
            "offset": "@audit.move", "clip_direction": "left",
            "anims": ["@audit.grow", "@audit.tint", "@audit.pan", "@audit.reveal"] }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.5);
    assert!(
        (drawn.dest.x - 10.0).abs() < 0.3,
        "offset x {}",
        drawn.dest.x
    );
    assert!((drawn.dest.h - 15.0).abs() < 0.3, "size h {}", drawn.dest.h);
    assert!(
        (drawn.dest.w - 10.0).abs() < 0.3,
        "clip keeps half of 20: {}",
        drawn.dest.w
    );
    let color = drawn.color.expect("tinted");
    assert!((i32::from(color[0]) - 128).abs() <= 3 && (i32::from(color[1]) - 128).abs() <= 3);
    let uv = drawn.uv.expect("panned");
    assert!((uv.u0 - 8.0 / 32.0).abs() < 0.01, "uv {uv:?}");
}

// N7/N17: a standalone wait destroys its named ancestor, siblings included.
#[test]
fn wait_destroys_the_named_ancestor() {
    let nodes = draws(
        r#""delay": { "anim_type": "wait", "duration": 1, "destroy_at_end": "dialog",
            "end_event": "button.gone" },
        "dialog": { "type": "panel", "size": [50, 50], "controls": [
            { "button": { "type": "image", "texture": "textures/ui/test", "size": [10, 10],
                "anims": ["@audit.delay"] } },
            { "title": { "type": "image", "texture": "textures/ui/test", "size": [10, 10] } } ] }"#,
        "dialog",
    );
    let mut animator = Animator::new();
    assert!(!play(&mut animator, node(&nodes, "button"), 0.0, 0.5).hidden);
    assert!(play(&mut animator, node(&nodes, "button"), 0.5, 1.2).hidden);
    assert_eq!(
        animator.take_events(),
        [
            AnimEvent::End("button.gone".into()),
            AnimEvent::Destroy("/dialog".into())
        ]
    );
    assert!(animator.is_destroyed(&node(&nodes, "title").key));
}

// N8/N30/N32: a flip-book in `anims` steps texture-width/count frames and turns
// around with repeated endpoints.
#[test]
fn flip_book_frames_follow_the_texture() {
    let nodes = draws(
        r#""frames": { "anim_type": "flip_book", "initial_uv": [0, 0], "frame_count": 4,
            "frame_step": 0, "fps": 10, "reversible": true },
        "image": { "type": "image", "texture": "textures/ui/strip", "size": [8, 8],
            "uv_size": [8, 8], "anims": ["@audit.frames"] }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    let mut frames = Vec::new();
    for tick in 0..10 {
        let drawn = image.animate(&mut animator, f64::from(tick) * 0.1001, None, Some(&Strip));
        frames.push((drawn.uv.expect("frame").u0 * 4.0).round() as i32);
    }
    assert_eq!(frames, [0, 1, 2, 3, 3, 2, 1, 0, 0, 1]);
    let uv = image
        .animate(&mut animator, 1.2, None, Some(&Strip))
        .uv
        .unwrap();
    assert!(
        (uv.u1 - uv.u0 - (8.0 - 0.0078125) / 32.0).abs() < 1e-5,
        "frame shrink"
    );
}

// N28: one tick of half a second advances one frame, keeping the rest.
#[test]
fn flip_book_advances_one_frame_per_tick() {
    let nodes = draws(
        r#""frames": { "anim_type": "flip_book", "frame_count": 8, "fps": 10 },
        "image": { "type": "image", "texture": "textures/ui/strip", "size": [8, 8],
            "uv_size": [4, 8], "uv": "@audit.frames" }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    let frame = |drawn: Animated| (drawn.uv.unwrap().u0 * 8.0).round() as i32;
    image.animate(&mut animator, 0.0, None, Some(&Strip));
    assert_eq!(
        frame(image.animate(&mut animator, 0.5, None, Some(&Strip))),
        1
    );
    assert_eq!(
        frame(image.animate(&mut animator, 0.51, None, Some(&Strip))),
        2
    );
}

// N29/N31/N27: an integer count is kept, vertical books step the height, and
// frames reset the cross-axis origin.
#[test]
fn flip_book_count_and_orientation() {
    let nodes = draws(
        r#""frames": { "anim_type": "flip_book", "initial_uv": [8, 16], "frame_count": 4,
            "fps": 10, "orientation": "vertical" },
        "image": { "type": "image", "texture": "textures/ui/tall", "size": [8, 8],
            "uv_size": [8, 8], "uv": "@audit.frames" }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    image.animate(&mut animator, 0.0, None, Some(&Strip));
    let uv = image
        .animate(&mut animator, 0.1, None, Some(&Strip))
        .uv
        .unwrap();
    assert_eq!(uv.u0, 0.0, "frames assign the origin");
    assert!((uv.v0 - (8.0 + 0.00390625) / 32.0).abs() < 1e-6, "{uv:?}");
    let many = draws(
        r#""frames": { "anim_type": "flip_book", "frame_count": 5000, "fps": 10 },
        "image": { "type": "image", "texture": "textures/ui/strip", "uv": "@audit.frames" }"#,
        "image",
    );
    let graph = many.iter().find_map(|node| node.anim.clone()).unwrap();
    assert_eq!(graph.own.as_ref().unwrap().graph.nodes[0].frame_count, 5000);
}

// N33: a non-looping book that cannot reset finishes and reports its end.
#[test]
fn non_looping_flip_book_ends() {
    let nodes = draws(
        r#""frames": { "anim_type": "flip_book", "frame_count": 2, "fps": 10, "looping": false,
            "resettable": false, "end_event": "button.done" },
        "image": { "type": "image", "texture": "textures/ui/strip", "uv": "@audit.frames" }"#,
        "image",
    );
    let mut animator = Animator::new();
    play(&mut animator, node(&nodes, "image"), 0.0, 0.5);
    assert_eq!(
        animator.take_events(),
        [AnimEvent::End("button.done".into())]
    );
}

// N9: an aseprite book picks the frame whose duration spans the elapsed time.
#[test]
fn aseprite_flip_book_uses_sheet_frames() {
    let nodes = draws(
        r#""frames": { "anim_type": "aseprite_flip_book", "initial_uv": [0, 0] },
        "image": { "type": "image", "texture": "textures/ui/sheet", "size": [16, 16],
            "uv_size": [8, 8], "uv": "@audit.frames" }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    let first = play(&mut animator, image, 0.0, 0.5).uv.unwrap();
    assert!((first.u0 - 10.0 / 16.0).abs() < 1e-5);
    let second = play(&mut animator, image, 0.5, 1.05).uv.unwrap();
    assert!((second.u0 - 84.0 / 16.0).abs() < 1e-5);
}

// N10/N11/N24/N25/N26: substituted ends, the 1 s default duration, inline
// definitions, inherited definitions and namespace-local references.
#[test]
fn definitions_resolve_like_the_factory() {
    let cases = [
        r#""fade": { "anim_type": "alpha", "from": "$start", "to": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "$start": 0,
            "alpha": "@audit.fade" }"#,
        r#""image": { "type": "image", "texture": "textures/ui/test",
            "alpha": { "anim_type": "alpha", "from": 0, "to": 1, "duration": 1 } }"#,
        r#""image": { "type": "image", "texture": "textures/ui/test",
            "anims": [{ "anim_type": "alpha", "from": 0, "to": 1, "duration": 1 }] }"#,
        r#""base": { "anim_type": "alpha", "from": 0, "to": 0, "duration": 1 },
        "fade@audit.base": { "to": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@audit.fade" }"#,
        r#""fade": { "anim_type": "alpha", "from": 0, "to": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@fade" }"#,
    ];
    for body in cases {
        let nodes = draws(body, "image");
        let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.5);
        assert!(close(drawn.opacity, 0.5), "{body}: {}", drawn.opacity);
    }
}

// N12/N13: long chains keep going, and a cycle re-enters at its entry, not the start.
#[test]
fn next_chains_link_instances() {
    let steps: Vec<String> = (0..17)
        .map(|index| {
            let next = if index < 16 {
                format!(r#", "next": "@audit.s{}""#, index + 1)
            } else {
                String::new()
            };
            let to = if index == 16 { 0.0 } else { 1.0 };
            format!(
                r#""s{index}": {{ "anim_type": "alpha", "from": 1, "to": {to}, "duration": 0.1{next} }}"#
            )
        })
        .collect();
    let body = format!(
        r#"{}, "image": {{ "type": "image", "texture": "textures/ui/test", "anims": ["@audit.s0"] }}"#,
        steps.join(",")
    );
    let nodes = draws(&body, "image");
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 2.0);
    assert!(
        close(drawn.opacity, 0.0),
        "17th step ran: {}",
        drawn.opacity
    );

    let nodes = draws(
        r#""intro": { "anim_type": "alpha", "from": 0, "to": 1, "duration": 1, "next": "@audit.pause" },
        "pause": { "anim_type": "wait", "duration": 1, "next": "@audit.out" },
        "out": { "anim_type": "alpha", "from": 1, "to": 0, "duration": 1, "next": "@audit.pause" },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@audit.intro" }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 3.5);
    assert!(
        close(drawn.opacity, 0.0),
        "pause after out holds 0: {}",
        drawn.opacity
    );
}

// N14/N15/N16: play and reset events drive an animation; its end is reported.
#[test]
fn events_play_reset_and_end() {
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 1, "to": 0, "duration": 1,
            "play_event": "button.start", "reset_event": "button.reset",
            "end_event": "button.finished" },
        "image": { "type": "image", "texture": "textures/ui/test", "anims": ["@audit.fade"] }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    assert!(
        close(play(&mut animator, image, 0.0, 1.0).opacity, 1.0),
        "waits"
    );
    animator.fire("button.start");
    assert!(close(play(&mut animator, image, 1.0, 1.5).opacity, 0.5));
    animator.fire("button.reset");
    assert!(
        close(play(&mut animator, image, 1.5, 2.0).opacity, 1.0),
        "reset waits at from"
    );
    animator.fire("button.start");
    play(&mut animator, image, 2.0, 3.2);
    assert_eq!(
        animator.take_events(),
        [AnimEvent::End("button.finished".into())]
    );
}

// N19/N20: a reset name restarts resettable animations and drops the others.
#[test]
fn animation_reset_name_restarts_resettable_animations() {
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 1, "to": 0, "duration": 1 },
        "keep": { "anim_type": "offset", "from": [0, 0], "to": [10, 0], "duration": 1,
            "resettable": false },
        "image": { "type": "image", "texture": "textures/ui/test", "size": [10, 10],
            "anims": ["@audit.fade", "@audit.keep"],
            "animation_reset_name": "screen_animation_reset" }"#,
        "image",
    );
    let image = node(&nodes, "image");
    let mut animator = Animator::new();
    let before = play(&mut animator, image, 0.0, 0.5);
    animator.fire("screen_animation_reset");
    let after = play(&mut animator, image, 0.5, 0.75);
    assert!(
        close(after.opacity, 0.75),
        "fade restarted: {}",
        after.opacity
    );
    assert_eq!(after.dest.x, before.dest.x, "non-resettable offset stopped");
}

// N18/N21/N22: creation clocks — a new control starts its own clock, a factory
// birth fast-forwards unless disabled, and render-waiting animations skip it.
#[test]
fn creation_clocks_follow_the_lifecycle() {
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 0, "to": 1, "duration": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@audit.fade" }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 10.0, 10.5);
    assert!(
        close(drawn.opacity, 0.5),
        "created at 10 s: {}",
        drawn.opacity
    );
    let born = |extra: &str| {
        draws(
            &format!(
                r#""fade": {{ "anim_type": "alpha", "from": 0, "to": 1, "duration": 1 {extra} }},
                "image": {{ "type": "image", "texture": "textures/ui/test", "anim_born": 0,
                    "alpha": "@audit.fade" {} }}"#,
                if extra.contains("disable") {
                    r#", "disable_anim_fast_forward": true"#
                } else {
                    ""
                }
            ),
            "image",
        )
    };
    let caught_up = born("");
    let drawn = node(&caught_up, "image").animate(&mut Animator::new(), 0.5, None, None);
    assert!(
        close(drawn.opacity, 0.5),
        "fast-forwarded: {}",
        drawn.opacity
    );
    let waiting = born(r#", "wait_until_rendered_to_play": true"#);
    let drawn = node(&waiting, "image").animate(&mut Animator::new(), 0.5, None, None);
    assert!(
        close(drawn.opacity, 0.0),
        "waits for its first paint: {}",
        drawn.opacity
    );
    let disabled = born(r#", "disable": 1"#);
    let drawn = node(&disabled, "image").animate(&mut Animator::new(), 0.5, None, None);
    assert!(
        close(drawn.opacity, 0.0),
        "no fast-forward: {}",
        drawn.opacity
    );
}

// N23: the property bag's wait scaler shortens a wait.
#[test]
fn wait_duration_scaler_scales_waits() {
    let nodes = draws(
        r#""wait": { "anim_type": "wait", "duration": 2, "next": "@audit.fade" },
        "fade": { "anim_type": "alpha", "from": 1, "to": 0, "duration": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@audit.wait",
            "property_bag": { "wait_duration_scaler": 0.5 } }"#,
        "image",
    );
    let drawn = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 1.5);
    assert!(close(drawn.opacity, 0.5), "{}", drawn.opacity);
}

// A sprite-less control still takes its alpha; paint only sees the sprite rules.
#[test]
fn text_draws_fade_through_propagated_alpha() {
    let nodes = draws(
        r#""fade": { "anim_type": "alpha", "from": 1, "to": 0, "duration": 1 },
        "panel": { "type": "panel", "size": [20, 20], "propagate_alpha": true,
            "anims": ["@audit.fade"], "controls": [
            { "child": { "type": "image", "texture": "textures/ui/test", "size": [5, 5] } } ] }"#,
        "panel",
    );
    let child = node(&nodes, "child");
    assert!(matches!(child.draw, Draw::Sprite { .. }));
    let drawn = play(&mut Animator::new(), child, 0.0, 0.5);
    assert!(close(drawn.opacity, 0.5), "{}", drawn.opacity);
}

// A zero-duration step lands on its `to` at once, even on a zero-delta paint.
#[test]
fn zero_duration_steps_finish_immediately() {
    let nodes = draws(
        r#""show": { "anim_type": "alpha", "duration": 0, "from": 0, "to": 1 },
        "image": { "type": "image", "texture": "textures/ui/test", "alpha": "@audit.show" }"#,
        "image",
    );
    let drawn = node(&nodes, "image").animate(&mut Animator::new(), 5.0, None, None);
    assert!(close(drawn.opacity, 1.0), "{}", drawn.opacity);
}

#[test]
fn review_offscreen_entrance_animation_survives_emission() {
    let nodes = draws(
        r#""move": {"anim_type":"offset","from":[-20,0],"to":[20,0],"duration":1},
        "image":{"type":"image","texture":"textures/ui/strip","size":[10,10],
        "anchor_from":"top_left","anchor_to":"top_left","offset":"@audit.move"}"#,
        "image",
    );
    assert_eq!(nodes.len(), 1);
    let animated = play(&mut Animator::new(), node(&nodes, "image"), 0.0, 0.75);
    assert!(animated.dest.x >= 0.0 && animated.dest.x < 20.0);
}
