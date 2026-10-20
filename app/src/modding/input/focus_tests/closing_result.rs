//! Deferred guest reads must survive the presentation lifetime of a completed HUD save.

use super::*;

/// Adds a guest result reader whose visible label distinguishes Save from Cancel.
fn fixture_with_result_reader() -> String {
    fixture()
        .replace(
            "(export \"set-label\"",
            r#"(type $p (record (field "x" float32) (field "y" float32)))
            (export "point" (type $point (eq $p)))
            (type $v (record (field "id" string) (field "position" (option $point))))
            (export "placement" (type $placement (eq $v)))
            (type $r (record (field "saved" bool) (field "reset" bool)
                (field "placements" (list $placement))))
            (export "editor-result" (type $editor-result (eq $r)))
            (export "read-editor-result" (func (result (result (option $editor-result) (error string)))))
            (export "set-label""#,
        )
        .replace(
            "(alias export $hud \"set-label\"",
            "(alias export $hud \"read-editor-result\" (func $read-result)) (alias export $hud \"set-label\"",
        )
        .replace(
            "(core func $lower-label",
            "(core func $lower-result (canon lower (func $read-result) (memory $memory) (realloc $realloc))) (core func $lower-label",
        )
        .replace(
            "(import \"host\" \"label\"",
            "(import \"host\" \"result\" (func $result (param i32))) (import \"host\" \"label\"",
        )
        .replace(
            "(export \"label\" (func $lower-label))",
            "(export \"result\" (func $lower-result)) (export \"label\" (func $lower-label))",
        )
        .replace(
            "(data (i32.const 0)",
            "(data (i32.const 16) \"Saved\") (data (i32.const 32) \"Cancelled\") (data (i32.const 0)",
        )
        .replace(
            "(func (export \"frame\") )",
            r#"(func (export "frame")
                i32.const 512 call $result
                i32.const 520 i32.load8_u
                if (result i32 i32)
                    i32.const 16 i32.const 5
                else
                    i32.const 32 i32.const 9
                end
                i32.const 600 call $label)"#,
        )
}

/// Leaves a successful result unread across dismissal and the complete closing animation.
fn assert_successful_save_survives_close(checkpoint_before_close: bool) {
    let (mut app, window) = personal_panel_app_with_fixture(false, &fixture_with_result_reader());
    let preview: ui::mod_hud::Hud = serde_json::from_value(serde_json::json!({
        "autosave":true,
        "cards":[{"id":"equipment","position":[0.5,0.5],"rows":[{"label":"Helmet","value":"85%"}]}],
        "surface":{
            "screen":"fixture.root",
            "document":serde_json::json!({"namespace":"fixture","root":{
                "type":"screen","size":["100%","100%"],"controls":[]
            }}).to_string(),
            "bindings":{"#alpha":1},
            "transitions":{"#alpha":{"duration":0.2,"exit":0}}
        }
    }))
    .unwrap();
    app.world_mut()
        .resource_mut::<ModRuntime>()
        .hud_editor_owner = Some(super::super::super::hud_editor::Owner {
        host: 0,
        session: 1,
    });
    app.world_mut()
        .resource_mut::<UiPresentationResource>()
        .mod_ui_mut()
        .open_mod_hud_editor(&preview)
        .unwrap();
    let paint_at = |app: &mut App, millis| {
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .build(
                &player_state::PlayerState::new(1),
                &UiRuntime::new(1),
                millis,
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
            )
            .unwrap();
    };
    paint_at(&mut app, 1000);
    if checkpoint_before_close {
        app.world_mut()
            .resource_scope(|world, mut p: Mut<UiPresentationResource>| {
                p.mod_ui_mut().mod_panel_events([640., 360.], true, false);
                p.mod_ui_mut().mod_panel_key("ArrowRight", None);
                assert!(
                    p.mod_ui().mod_hud_editor_state().unwrap().draft.cards[0]
                        .position
                        .unwrap()[0]
                        > 0.5
                );
                let mut runtime = world.resource_mut::<ModRuntime>();
                super::super::super::hud_editor::collect(&mut runtime, &mut p);
                assert!(p.mod_ui_mut().take_mod_hud_editor_result().is_none());
            });
    }
    app.world_mut().write_message(KeyboardInput {
        key_code: if checkpoint_before_close {
            KeyCode::ShiftRight
        } else {
            KeyCode::Enter
        },
        logical_key: if checkpoint_before_close {
            Key::Shift
        } else {
            Key::Enter
        },
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_hud_editor_painting()
    );
    paint_at(&mut app, 1100);
    paint_at(&mut app, 1400);
    assert!(
        !app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_hud_editor_painting()
    );
    app.update();
    let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
    assert!(runtime.hud_editor_owner.is_none());
    let host = runtime.hosts.host_mut(0);
    host.frame(false).unwrap();
    assert_eq!(host.label(), Some("Saved"));
}

#[test]
fn completed_hud_exit_does_not_replace_an_unread_successful_save() {
    assert_successful_save_survives_close(false);
}

#[test]
fn normal_hud_close_preserves_an_unread_autosave_checkpoint() {
    assert_successful_save_survives_close(true);
}
