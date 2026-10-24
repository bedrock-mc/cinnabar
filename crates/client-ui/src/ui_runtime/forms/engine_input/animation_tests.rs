use crate::ui_runtime::presentation::forms::{
    ServerUiPack, pack_harness, snapshot, tests::mini_engine_presentation,
};

#[test]
fn an_unpainted_state_child_cannot_finish_its_animation_through_hit_projection() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = pack_harness::action_form(&mut player, "Stateful choice", &["Choose"]);
    let mut presentation = mini_engine_presentation();
    presentation.set_gui_scale_preference(Some(1));
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            br#"{
                "namespace":"server_form",
                "finish":{"anim_type":"offset","from":[0,0],"to":[10,0],"duration":1,
                    "wait_until_rendered_to_play":true,"end_event":"button.hidden_animation_finished"},
                "main_screen_content":{"type":"panel","size":["100%","100%"],
                    "controls":[{"server_form_factory":{"type":"factory","control_ids":{
                        "long_form":"@server_form.stateful_menu"
                    }}}]},
                "stateful_menu":{"type":"panel","size":[200,100],"controls":[
                    {"outer":{"type":"button","size":[100,50],
                        "default_control":"normal","hover_control":"over","controls":[
                            {"normal":{"type":"button","size":[50,20],
                                "offset":"@server_form.finish","controls":[
                                    {"face":{"type":"image","size":["100%","100%"],"color":[0,1,0,1]}}
                                ]}},
                            {"over":{"type":"image","size":["100%","100%"],"color":[0,0,1,1]}}
                        ]}}
                ]}
            }"#
            .to_vec(),
        )]],
        ..Default::default()
    });
    let neutral = presentation
        .build(
            &player,
            &runtime,
            0,
            [200, 100],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        snapshot::rasterize(&neutral)
            .pixels()
            .any(|pixel| pixel.0 == [0, 255, 0, 255]),
        "the default state's real draw starts its animation"
    );
    let identity = runtime.server_forms().active().unwrap().identity;
    let outer = presentation
        .form_engine_frame(identity)
        .unwrap()
        .hits
        .iter()
        .find(|region| region.name == "outer")
        .unwrap()
        .key
        .clone();
    runtime.server_forms_mut().engine_mut().view.hovered = Some(outer);
    let mut events = presentation.form_animator().unwrap().take_events();
    for millis in [100, 1_200] {
        let input = presentation
            .build(
                &player,
                &runtime,
                millis,
                [200, 100],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let image = snapshot::rasterize(&input);
        assert!(image.pixels().any(|pixel| pixel.0 == [0, 0, 255, 255]));
        assert!(
            image.pixels().all(|pixel| pixel.0 != [0, 255, 0, 255]),
            "only the hover state paints at caller time {millis}"
        );
        events.extend(presentation.form_animator().unwrap().take_events());
    }
    assert!(
        !events.iter().any(|event| matches!(event,
            json_ui::AnimEvent::End(name) if name == "button.hidden_animation_finished"
        )),
        "hit projection must not tick or retain an unpainted state child's animation: {events:?}"
    );
}
