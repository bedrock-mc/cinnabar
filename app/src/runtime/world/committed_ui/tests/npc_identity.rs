use super::*;

/// Submits a live NPC and a dialogue whose persistent ID differs from its runtime ID.
fn submit_npc_dialogue(app: &mut App, actor_present: bool) {
    let mut world = app.world_mut().resource_mut::<ClientWorld>();
    let stream = world.stream.as_mut().unwrap();
    if actor_present {
        stream
            .submit(
                1,
                WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                    dimension: 0,
                    unique_id: -77,
                    runtime_id: 99,
                    kind: protocol::ActorKind::Entity {
                        identifier: "minecraft:npc".into(),
                    },
                    position: [0.0, 70.0, 0.0],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    } else {
        stream.commit(1).unwrap();
    }
    stream
        .submit(
            2,
            WorldEvent::Ui(UiEvent::Form(FormRequestEvent {
                form_id: protocol::NPC_DIALOGUE_FORM_ID,
                kind: FormKind::NpcDialogue,
                title: Some("Trader".into()),
                json: "[]".into(),
                model: ServerFormModel::NpcDialogue(protocol::NpcDialogueForm {
                    npc_unique_id: -77,
                    scene_name: "shop".into(),
                    npc_name: "Trader".into(),
                    dialogue: "Choose".into(),
                    buttons: Arc::from([]),
                    open: true,
                }),
            })),
        )
        .unwrap();
}

#[test]
fn committed_npc_dialogue_binds_its_original_live_actor_for_replies() {
    let (mut app, _) = fixture_app();
    submit_npc_dialogue(&mut app, true);
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    schedule.run(app.world_mut());
    let lifetime = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_lifetime(99)
        .unwrap();
    let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime
        .respond_to_server_form(identity, LocalFormAction::Dismiss)
        .unwrap();
    let mut sent = Vec::new();
    assert!(
        flush_form_response(
            &mut runtime,
            |actor| actor == lifetime,
            |packet| {
                sent.push(protocol::encode(&packet).unwrap());
                Ok(())
            }
        )
        .unwrap()
    );
    assert_eq!(
        sent,
        [protocol::encode(&protocol::npc_request_packet(
            99,
            "shop",
            protocol::NpcRequestKind::ExecuteClosingCommands
        ))
        .unwrap()]
    );
}

#[test]
fn committed_npc_dialogue_without_an_actor_is_not_admitted() {
    let (mut app, _) = fixture_app();
    submit_npc_dialogue(&mut app, false);
    let mut schedule = Schedule::default();
    schedule.add_systems(drain_committed_ui_before_authority);
    schedule.run(app.world_mut());
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .server_forms()
            .active()
            .is_none()
    );
}
