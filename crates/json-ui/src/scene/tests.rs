use serde_json::json;

use super::*;

fn settings(value: Value) -> ScreenSettings {
    let Value::Object(map) = value else {
        panic!("object");
    };
    ScreenSettings::from_properties(&map.into_iter().collect())
}

fn gameplay() -> ScreenSettings {
    settings(json!({"absorbs_input": false, "is_showing_menu": false,
        "should_steal_mouse": true, "render_only_when_topmost": false}))
}

fn hud() -> ScreenSettings {
    settings(json!({"is_showing_menu": false, "should_steal_mouse": true,
        "low_frequency_rendering": true, "absorbs_input": false}))
}

// Omitted settings take the parser's defaults, and an unbound `$var` is not a bool.
#[test]
fn omitted_and_unbound_settings_take_parser_defaults() {
    let parsed = settings(json!({"force_render_below": "$force_render_below"}));
    assert_eq!(parsed, ScreenSettings::default());
    assert!(parsed.render_game_behind && parsed.absorbs_input && parsed.is_showing_menu);
    assert!(parsed.send_telemetry && parsed.render_only_when_topmost);
    assert!(!parsed.is_modal && !parsed.should_steal_mouse && !parsed.force_render_below);
}

// With only the HUD over gameplay, clicks reach gameplay and the mouse is captured.
#[test]
fn hud_passes_input_through_to_gameplay() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("hud", hud());
    assert_eq!(stack.input_targets(), ["hud", "gameplay"]);
    assert!(stack.receives_input("gameplay"));
    assert!(stack.steals_mouse());
    assert!(!stack.showing_menu());
}

// A default (absorbing, menu) screen over the HUD takes input and frees the mouse.
#[test]
fn absorbing_screen_takes_input_from_gameplay() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("hud", hud());
    stack.push("pause", ScreenSettings::default());
    assert_eq!(stack.input_targets(), ["pause"]);
    assert!(!stack.receives_input("gameplay"));
    assert!(!stack.steals_mouse());
    assert!(stack.showing_menu());
}

// A screen that passes input on still lets the scene below see it.
#[test]
fn non_absorbing_overlay_forwards_input() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("overlay", settings(json!({"absorbs_input": false})));
    assert!(stack.receives_input("gameplay"));
    assert!(!stack.steals_mouse());
}

// Always-accept scenes under an absorbing top still receive input.
#[test]
fn always_accepting_scene_receives_input_under_an_absorber() {
    let mut stack = SceneStack::default();
    stack.push("toast", settings(json!({"always_accepts_input": true})));
    stack.push("pause", ScreenSettings::default());
    assert_eq!(stack.input_targets(), ["pause", "toast"]);
}

// A topmost-only HUD hides under a menu but stays under a force-render-below dialog.
#[test]
fn topmost_only_scene_hides_unless_the_top_renders_below() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("hud", hud());
    stack.push("chat", ScreenSettings::default());
    assert_eq!(stack.visible(false), ["gameplay", "chat"]);

    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("hud", hud());
    stack.push("dialog", settings(json!({"force_render_below": true})));
    assert_eq!(stack.visible(false), ["gameplay", "hud", "dialog"]);
}

// Scenes under the highest one that hides the game are not drawn.
#[test]
fn opaque_scene_hides_everything_below() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("opaque", settings(json!({"render_game_behind": false})));
    stack.push("dialog", settings(json!({"force_render_below": true})));
    assert_eq!(stack.visible(false), ["opaque", "dialog"]);
}

// Draws-last scenes paint after the rest, even below an opaque scene.
#[test]
fn draws_last_scenes_paint_in_a_second_pass() {
    let mut stack = SceneStack::default();
    stack.push(
        "debug",
        settings(json!({"screen_draws_last": true, "render_only_when_topmost": false})),
    );
    stack.push("gameplay", gameplay());
    stack.push("opaque", settings(json!({"render_game_behind": false})));
    assert_eq!(stack.visible(false), ["opaque", "debug"]);
}

// The per-frame pass skips menu and low-frequency scenes.
#[test]
fn frequent_pass_skips_menus_and_low_frequency_scenes() {
    let mut stack = SceneStack::default();
    stack.push("gameplay", gameplay());
    stack.push("hud", hud());
    assert_eq!(stack.visible(true), ["gameplay"]);
}

#[test]
fn close_on_hurt_reports_only_the_top_scene() {
    let mut stack = SceneStack::default();
    stack.push("chest", settings(json!({"close_on_player_hurt": true})));
    assert_eq!(stack.closes_on_hurt(), Some("chest"));
    stack.push("pause", ScreenSettings::default());
    assert_eq!(stack.closes_on_hurt(), None);
}

// Pushes then pops restore earlier screens in order.
#[test]
fn nav_pops_restore_pushed_screens() {
    let mut nav = ScreenNav::default();
    nav.reset("a");
    nav.push("b");
    nav.push("c");
    assert_eq!(nav.pop(), Some("c"));
    assert_eq!(nav.top(), Some("b"));
    assert!(nav.pop_back_to("a"));
    assert_eq!(nav.screens(), ["a"]);
    assert!(!nav.pop_back_to("z"));
}

// Scheduled operations wait for update; an expected-name pop that does not match is dropped.
#[test]
fn scheduled_pops_apply_on_update_and_check_names() {
    let mut nav = ScreenNav::default();
    nav.reset("a");
    nav.push("b");
    nav.schedule_pop_expecting(vec!["c"]);
    nav.schedule_push("d");
    assert_eq!(nav.top(), Some("b"));
    nav.update();
    assert_eq!(nav.screens(), ["a", "b", "d"]);
    nav.schedule_pop_expecting(vec!["d", "b"]);
    nav.update();
    assert_eq!(nav.screens(), ["a"]);
}

// Scheduled pops never exceed the screens available to pop.
#[test]
fn scheduled_pops_are_bounded() {
    let mut nav = ScreenNav::default();
    nav.reset("a");
    nav.schedule_pop(5);
    nav.schedule_pop(1);
    nav.update();
    assert!(nav.screens().is_empty());
}

// A flush keeps non-flushable screens.
#[test]
fn flush_keeps_unflushable_screens() {
    let mut nav = ScreenNav::default();
    nav.reset("toast");
    nav.push("a");
    nav.push("b");
    nav.flush(|screen| screen == "toast");
    assert_eq!(nav.screens(), ["toast"]);
}
