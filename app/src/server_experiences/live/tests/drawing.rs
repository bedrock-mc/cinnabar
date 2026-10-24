use super::*;

#[test]
fn actor_leaving_candidates_during_a_callback_keeps_its_live_reference() {
    let mut live = initialized(1);
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let mut observed = context(1);
    observed.candidates.push(actor);
    observed.live.push(actor);
    live.bind_drawing(&observed, |_| true).unwrap();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .dispatch(None, Event::Frame, 1, None)
        .unwrap();
    assert!(
        live.instances["bundle0"]
            .presentation
            .output()
            .0
            .retained
            .is_empty()
    );
    live.bind_drawing(&context(1), |key| key == actor).unwrap();
    assert!(
        live.instances["bundle0"]
            .drawing_context
            .live
            .contains(&actor)
    );
    live.bind_drawing(&context(1), |_| false).unwrap();
    assert!(
        !live.instances["bundle0"]
            .drawing_context
            .live
            .contains(&actor)
    );
}

#[test]
fn restarted_initializer_references_stay_live_while_dispatch_pauses() {
    let mut live = initialized(1);
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let mut observed = context(1);
    observed.candidates.push(actor);
    observed.live.push(actor);
    live.bind_drawing(&observed, |_| true).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let helper = instance.helper.as_mut().unwrap();
    helper.references = vec![actor];
    helper.response = None;
    helper.failure = Some(CallFailure {
        bundle: "bundle0".into(),
        callback: "frame".into(),
        kind: FailureKind::Panic,
        reason: "fixture restart".into(),
        fuel: Some(1),
    });
    live.bind_drawing(&context(1), |_| true).unwrap();
    live.poll(1, CALLBACK_INTERVAL_MS, false, |_| true).unwrap();
    live.bind_drawing(&context(1), |_| true).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert!(instance.stopped.is_none());
    assert!(!instance.busy);
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let next = instance.helper.as_ref().unwrap().contexts.last().unwrap();
    assert!(next.candidates.is_empty());
    assert_eq!(next.live, [actor]);
}

#[test]
fn held_actor_references_survive_candidate_turnover_during_snapshot_transfer() {
    let mut live = initialized(1);
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let mut observed = context(1);
    observed.candidates.push(actor);
    observed.live.push(actor);
    live.bind_drawing(&observed, |_| true).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.capabilities.scope.permissions.extend([
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::RenderThroughWalls,
    ]);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let helper = instance.helper.as_mut().unwrap();
    helper.references = vec![actor];
    let mut output = mod_host::presentation::Output::default();
    output.drawing.entities.push((
        mod_api::drawing::DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        actor,
    ));
    let bytes = serde_json::to_vec(&output).unwrap();
    helper.presentation = bytes
        .chunks(32)
        .enumerate()
        .map(|(index, part)| mod_host::presentation::Chunk {
            revision: 1,
            offset: (index * 32) as u32,
            total: bytes.len() as u32,
            bytes: part.to_vec(),
        })
        .collect();
    let pieces = helper.presentation.len();
    for slice in 1..=pieces + mod_api::MAX_DRAW_ENTITIES * 3 {
        let other = mod_render::retained::EntityKey {
            runtime_id: 100 + slice as u64,
            ..actor
        };
        let mut next = context(1);
        next.candidates.push(other);
        next.live.push(other);
        live.bind_drawing(&next, |_| true).unwrap();
        live.poll(1, slice as u64 * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
        let instance = &live.instances["bundle0"];
        assert!(instance.stopped.is_none());
        let dispatched = instance.helper.as_ref().unwrap().contexts.last().unwrap();
        assert!(dispatched.live.contains(&actor));
        assert!(!dispatched.candidates.contains(&actor));
        if slice < pieces {
            assert!(instance.presentation.output().0.retained.is_empty());
        }
    }
    assert_eq!(
        live.instances["bundle0"].presentation.output().0.retained[0]
            .entities
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [actor]
    );
    live.bind_drawing(&context(1), |_| false).unwrap();
    assert!(
        !live.instances["bundle0"]
            .drawing_context
            .live
            .contains(&actor)
    );
    let changed = mod_host::presentation::Context {
        dimension: 1,
        ..context(1)
    };
    live.bind_drawing(&changed, |_| true).unwrap();
    assert!(live.instances["bundle0"].drawing_context.live.is_empty());
    assert!(
        live.instances["bundle0"]
            .presentation
            .output()
            .0
            .retained
            .is_empty()
    );
}

#[test]
fn staggered_drawing_initializers_send_ready_before_overlapping_frames() {
    let mut live = fixture(2);
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
    }
    complete(&mut live, "bundle0");
    assert!(
        live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap()
            .is_empty()
    );
    // Let an early frame stay pending while the second initializer finishes.
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response = None;
    complete(&mut live, "bundle1");
    let packets = live
        .poll(1, 2 * CALLBACK_INTERVAL_MS, true, |_| true)
        .unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    assert!(live.ready);
    for instance in live.instances.values() {
        assert_eq!(instance.helper.as_ref().unwrap().dispatched, [Event::Frame]);
    }
}

/// Completes every startup callback across aggregate scheduling slices.
fn initialized(count: usize) -> Live<FakeWorker> {
    let mut live = fixture(count);
    for slice in 0..count {
        for instance in live.instances.values_mut() {
            if instance.busy
                && instance.callback == "init"
                && let Some(helper) = &mut instance.helper
            {
                helper.response = Some(Transaction {
                    owner: instance.owner.clone(),
                    epoch: live.epoch,
                    commands: Vec::new(),
                });
            }
        }
        live.poll(1, slice as u64 * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
    }
    assert!(live.ready);
    live
}

#[test]
fn frame_callbacks_share_the_session_budget_fairly() {
    let mut live = initialized(4);
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
    }
    for slice in 4..12 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
    }
    for instance in live.instances.values() {
        let helper = instance.helper.as_ref().unwrap();
        assert_eq!(
            helper.dispatched,
            vec![Event::Frame; 4],
            "{}",
            instance.owner.bundle
        );
    }
}

#[test]
fn every_multichunk_initial_scene_finishes_under_frame_budget_pressure() {
    let mut live = fixture(4);
    let bytes = serde_json::to_vec(&mod_host::presentation::Output {
        time: Some(18_000),
        ..Default::default()
    })
    .unwrap();
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
        instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    }
    for slice in 0..16 {
        for instance in live.instances.values_mut() {
            if instance.busy
                && instance.callback == "init"
                && let Some(helper) = &mut instance.helper
            {
                helper.presentation = bytes
                    .chunks(32)
                    .enumerate()
                    .map(|(index, part)| mod_host::presentation::Chunk {
                        revision: 1,
                        offset: (index * 32) as u32,
                        total: bytes.len() as u32,
                        bytes: part.to_vec(),
                    })
                    .collect();
                helper.response = Some(Transaction {
                    owner: instance.owner.clone(),
                    epoch: live.epoch,
                    commands: Vec::new(),
                });
            }
        }
        live.poll(1, slice * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
    }
    assert!(live.ready);
    for instance in live.instances.values() {
        assert_eq!(
            instance.presentation.time,
            Some(18_000),
            "{}",
            instance.owner.bundle
        );
        assert!(instance.helper.as_ref().unwrap().presentation.is_empty());
    }
}

#[test]
fn drawing_releases_retired_gpu_charges_without_a_helper_reply() {
    use mod_api::drawing::{DrawingHandle, DrawingPermissions};
    use mod_render::retained::{GeometrySpec, Vertex};

    let mut live = initialized(1);
    let mut input = mod_host::presentation::Output::default();
    input.drawing.geometry.push((
        DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        GeometrySpec::Triangles {
            vertices: [[0.; 3], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        },
    ));
    let context = mod_host::presentation::Context::default();
    live.instances
        .values_mut()
        .next()
        .unwrap()
        .presentation
        .apply(input, DrawingPermissions::ALL, &context, u64::MAX, |_| true)
        .unwrap();
    let ledger = live.drawing().0.gpu_reservations[0].ledger.clone();
    let gpu_mesh = ledger.allocate(256, || Some(())).unwrap();
    live.instances
        .values_mut()
        .next()
        .unwrap()
        .presentation
        .apply(
            Default::default(),
            DrawingPermissions::ALL,
            &context,
            u64::MAX,
            |_| true,
        )
        .unwrap();
    let (output, revision) = live.drawing();
    assert_eq!(output.gpu_reservations[0].ledger.used(), 256);
    drop(gpu_mesh);
    let (output, released_revision) = live.drawing();
    assert_eq!(output.gpu_reservations[0].ledger.used(), 0);
    assert_eq!(released_revision, revision);
    assert_eq!(live.drawing().1, released_revision);
}

#[test]
fn idle_dimension_change_binds_current_entities_before_epoch_callback() {
    let mut live = initialized(1);
    live.grant.wire = Wire {
        version: MAX_WIRE_VERSION,
        limits: Limits::host(),
    };
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.capabilities.scope.permissions.extend([
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::RenderThroughWalls,
    ]);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.helper.as_mut().unwrap().observe_epoch = true;
    let old = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let mut initial = context(1);
    initial.candidates = vec![old];
    initial.live = vec![old];
    initial.targets = vec![(
        mod_api::drawing::MaskEntity {
            connection: old.session,
            dimension: old.dimension,
            runtime_id: old.runtime_id,
            spawn_revision: old.spawn_revision,
        },
        3,
    )];
    live.bind_drawing(&initial, |_| true).unwrap();
    let current = mod_render::retained::EntityKey {
        dimension: 1,
        runtime_id: 43,
        ..old
    };
    let current_context = mod_host::presentation::Context {
        dimension: 1,
        candidates: vec![current],
        live: vec![current],
        targets: vec![(
            mod_api::drawing::MaskEntity {
                connection: current.session,
                dimension: current.dimension,
                runtime_id: current.runtime_id,
                spawn_revision: current.spawn_revision,
            },
            3,
        )],
        ..context(2)
    };
    live.bind_drawing(&current_context, |key| key == current)
        .unwrap();
    live.poll(2, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    live.poll(2, 2 * CALLBACK_INTERVAL_MS, true, |_| true)
        .unwrap();
    let instance = &live.instances["bundle0"];
    assert!(instance.stopped.is_none());
    let context = instance.helper.as_ref().unwrap().contexts.last().unwrap();
    assert_eq!(context, &current_context);
    assert_eq!(
        instance.presentation.output().0.retained[0]
            .entities
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [current]
    );
}

#[test]
fn pending_initialization_uses_its_original_drawing_fence() {
    let mut live = fixture(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::EntityHighlight);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    let helper = instance.helper.as_mut().unwrap();
    assert_eq!(helper.contexts, [context(1)]);
    let bytes = serde_json::to_vec(&mod_host::presentation::Output {
        time: Some(18_000),
        ..Default::default()
    })
    .unwrap();
    helper
        .presentation
        .push_back(mod_host::presentation::Chunk {
            revision: 1,
            offset: 0,
            total: bytes.len() as u32,
            bytes,
        });
    complete(&mut live, "bundle0");
    let changed = mod_host::presentation::Context {
        dimension: 1,
        ..context(1)
    };
    live.bind_drawing(&changed, |_| false).unwrap();
    live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    assert!(live.ready);
    assert_eq!(live.instances["bundle0"].presentation.time, None);
}

#[test]
fn dispatch_requires_a_context_for_the_callback_epoch() {
    let mut live = initialized(1);
    let instance = live.instances.get_mut("bundle0").unwrap();
    assert!(instance.dispatch(None, Event::Epoch, 2, None).is_err());
    instance.drawing_context = Default::default();
    assert!(instance.dispatch(None, Event::Frame, 1, None).is_err());
    assert!(instance.helper.as_ref().unwrap().dispatched.is_empty());
}

/// Queues one complete retained scene as though the helper had committed it.
fn scene(live: &mut Live<FakeWorker>, bundle: &str, count: usize) {
    use mod_render::retained::{self, Anchor, GeometrySpec, Vertex};
    let mut store = retained::Store::default();
    let geometry = store
        .create_geometry(GeometrySpec::Triangles {
            vertices: [[0.; 3], [1., 0., 0.], [0., 1., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
        .unwrap();
    let instance = retained::Instance {
        geometry,
        material: None,
        params: [0.; mod_api::materials::PARAMS],
        anchor: Anchor::Camera,
        transform: Default::default(),
        tint: [1.; 4],
        depth: retained::Depth::Test,
        blend: retained::Blend::Premultiplied,
        width: 1.,
        width_mode: retained::WidthMode::World,
        join: retained::Join::Bevel,
        cap: retained::Cap::Butt,
    };
    store
        .replace_group(
            "frame".into(),
            vec![instance; count],
            mod_api::drawing::DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    let instance = live.instances.get_mut(bundle).unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::WorldDraw);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    let bytes = serde_json::to_vec(&mod_host::presentation::Output {
        drawing: mod_render::retained::transfer::Snapshot::from_resources(store.snapshot()),
        ..Default::default()
    })
    .unwrap();
    instance.helper.as_mut().unwrap().presentation.extend(
        bytes
            .chunks(MAX_HOST_OUTPUT / 4)
            .enumerate()
            .map(|(index, part)| mod_host::presentation::Chunk {
                revision: 1,
                offset: (index * (MAX_HOST_OUTPUT / 4)) as u32,
                total: bytes.len() as u32,
                bytes: part.to_vec(),
            }),
    );
    complete(live, bundle);
}

#[test]
fn over_limit_group_stops_only_its_bundle_while_another_keeps_drawing() {
    let mut live = initialized(2);
    scene(&mut live, "bundle0", MAX_DRAWS as usize + 1);
    scene(&mut live, "bundle1", 1);
    let pieces = live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .presentation
        .len();
    for slice in 1..=pieces as u64 {
        live.poll(1, slice * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
    }
    assert!(live.instances["bundle0"].stopped.is_some());
    assert!(live.instances["bundle0"].helper.is_none());
    assert!(live.instances["bundle1"].helper.is_some());
    let (output, _) = live.drawing();
    assert_eq!(output.retained.len(), 1);
    assert_eq!(output.retained[0].instances.len(), 1);
    live.poll(1, (pieces as u64 + 1) * CALLBACK_INTERVAL_MS, true, |_| {
        true
    })
    .unwrap();
    assert!(live.instances["bundle1"].stopped.is_none());
    assert!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .len()
            >= 2
    );
}

#[test]
fn paused_dispatch_keeps_polling_completions_and_defers_all_new_work() {
    let mut live = initialized(2);
    for instance in live.instances.values_mut() {
        instance
            .capabilities
            .scope
            .permissions
            .insert(Permission::WorldDraw);
    }
    live.poll(1, 3 * CALLBACK_INTERVAL_MS, true, |_| true)
        .unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.events.push_back(Event::Epoch);
    assert!(instance.busy);
    live.poll(1, 10_000, false, |_| true).unwrap();
    for instance in live.instances.values() {
        assert!(!instance.busy);
        assert!(instance.stopped.is_none());
        assert_eq!(instance.helper.as_ref().unwrap().dispatched, [Event::Frame]);
    }
    assert_eq!(live.instances["bundle0"].events.len(), 1);
    live.poll(1, 20_000, false, |_| true).unwrap();
    assert_eq!(live.instances["bundle0"].events.len(), 1);
    live.poll(1, 20_000, true, |_| true).unwrap();
    assert_eq!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched,
        [Event::Frame, Event::Epoch]
    );
    assert_eq!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched,
        vec![Event::Frame; 2]
    );

    let mut live = fixture(3);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, 10_000, false, |_| true).unwrap();
    assert!(live.instances["bundle2"].helper.is_none());
    assert!(live.instances["bundle2"].component.is_some());
}

#[test]
fn helper_transport_and_dispatch_failures_do_not_stop_another_bundle() {
    for during_dispatch in [false, true] {
        let mut live = initialized(2);
        for instance in live.instances.values_mut() {
            instance
                .capabilities
                .scope
                .permissions
                .insert(Permission::WorldDraw);
        }
        let helper = live
            .instances
            .get_mut("bundle0")
            .unwrap()
            .helper
            .as_mut()
            .unwrap();
        helper.transport_error = !during_dispatch;
        helper.refuse_dispatch = during_dispatch;
        live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
        assert!(live.instances["bundle0"].stopped.is_some());
        assert!(live.instances["bundle1"].stopped.is_none());
        live.poll(1, 2 * CALLBACK_INTERVAL_MS, true, |_| true)
            .unwrap();
        assert_eq!(
            live.instances["bundle1"]
                .helper
                .as_ref()
                .unwrap()
                .dispatched,
            vec![Event::Frame; 2]
        );
    }
}

#[test]
fn rejected_drawing_decode_and_transactions_stop_only_their_producer() {
    for bad_drawing in [false, true] {
        let mut live = initialized(2);
        scene(&mut live, "bundle1", 1);
        complete(&mut live, "bundle0");
        live.instances
            .get_mut("bundle0")
            .unwrap()
            .capabilities
            .scope
            .memory_bytes = MAX_GUEST_MEMORY;
        let helper = live
            .instances
            .get_mut("bundle0")
            .unwrap()
            .helper
            .as_mut()
            .unwrap();
        if bad_drawing {
            helper
                .presentation
                .push_back(mod_host::presentation::Chunk {
                    revision: 1,
                    offset: 0,
                    total: 1,
                    bytes: vec![b'{'],
                });
        } else {
            helper.response.as_mut().unwrap().owner.bundle = "bundle1".into();
        }
        live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
        assert!(live.instances["bundle0"].stopped.is_some());
        assert!(live.instances["bundle1"].stopped.is_none());
        assert_eq!(live.drawing().0.retained[0].instances.len(), 1);
    }
}

#[test]
fn failed_launch_does_not_block_readiness_or_other_initializers() {
    let mut live = fixture(3);
    live.instances.get_mut("bundle2").unwrap().component = Some(vec![255]);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    let packets = live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    assert!(live.instances["bundle2"].stopped.is_some());
    assert!(live.instances["bundle0"].stopped.is_none());
    assert!(live.instances["bundle1"].stopped.is_none());
}

#[test]
fn routed_payload_rejection_preserves_other_bundles_and_the_reliable_sequence() {
    let mut live = initialized(2);
    let message = Envelope {
        version: live.grant.wire.version,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: live.grant.subclient,
        bundle: "bundle0".into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: "bundle0.events0".into(),
        schema: API_VERSION,
        sequence: 1,
        world_epoch: 1,
        payload: vec![Scalar::Integer(7)],
    };
    live.receive(&serde_json::to_vec(&message).unwrap(), 0)
        .unwrap();
    assert!(live.instances["bundle0"].stopped.is_some());
    receive(&mut live, "bundle1", 2);
    live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    assert!(live.instances["bundle1"].stopped.is_none());
    assert!(matches!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .last(),
        Some(Event::Message { .. })
    ));
    assert!(live.receive(b"{", 0).is_err());
}

#[test]
fn discarded_initialization_sends_do_not_leave_a_sequence_gap() {
    let mut live = fixture(2);
    complete(&mut live, "bundle0");
    live.poll(1, 0, true, |_| true).unwrap();
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .transport_error = true;
    complete(&mut live, "bundle1");
    let packets = live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    let send: Envelope = serde_json::from_slice(&packets[1]).unwrap();
    assert_eq!(send.bundle, "bundle1");
    assert_eq!(send.sequence, 1);
    assert_eq!(live.sequence, 2);
    assert!(live.pending_sends.is_empty());
    assert_eq!(live.pending_send_bytes, 0);
}

#[test]
fn guest_rate_exhaustion_cannot_reject_a_host_epoch_control() {
    let mut live = initialized(2);
    live.grant.wire = Wire {
        version: MAX_WIRE_VERSION,
        limits: Limits::host(),
    };
    for _ in 0..MAX_MESSAGES_PER_SECOND {
        live.egress.charge(1, 0).unwrap();
    }
    let packets = live.poll(2, 0, false, |_| true).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Epoch { world_epoch: 2, .. }
    ));
    assert!(
        live.instances
            .values()
            .all(|instance| instance.stopped.is_none())
    );
}

#[test]
fn a_rejected_commit_releases_shared_queue_space_before_the_next_bundle() {
    let mut live = initialized(2);
    scene(&mut live, "bundle1", 1);
    complete(&mut live, "bundle0");
    let transaction = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap()
        .response
        .as_mut()
        .unwrap();
    transaction.commands = vec![transaction.commands[0].clone(); MAX_QUEUE_MESSAGES + 1];
    assert!(serde_json::to_vec(transaction).unwrap().len() <= MAX_HOST_OUTPUT);
    let packets = live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    assert!(live.instances["bundle0"].stopped.is_some());
    assert!(live.instances["bundle1"].stopped.is_none());
    assert_eq!(live.drawing().0.retained[0].instances.len(), 1);
    assert_eq!(packets.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Envelope>(&packets[0])
            .unwrap()
            .bundle,
        "bundle1"
    );
    assert_eq!(live.pending_send_bytes, 0);
}

#[test]
fn stopped_bundles_release_input_queues_even_while_dispatch_is_paused() {
    for wire_failure in [false, true] {
        let mut live = initialized(2);
        receive(&mut live, "bundle0", 1);
        receive(&mut live, "bundle1", 2);
        let next = if wire_failure {
            let message = Envelope {
                version: live.grant.wire.version,
                session: live.grant.session.clone(),
                connection: live.grant.connection.clone(),
                subclient: live.grant.subclient,
                bundle: "bundle0".into(),
                generation: INITIAL_BUNDLE_GENERATION,
                channel: "bundle0.events0".into(),
                schema: API_VERSION,
                sequence: 3,
                world_epoch: 1,
                payload: vec![Scalar::Integer(7)],
            };
            live.receive(&serde_json::to_vec(&message).unwrap(), CALLBACK_INTERVAL_MS)
                .unwrap();
            4
        } else {
            live.instances
                .get_mut("bundle0")
                .unwrap()
                .helper
                .as_mut()
                .unwrap()
                .transport_error = true;
            live.poll(1, CALLBACK_INTERVAL_MS, false, |_| true).unwrap();
            3
        };
        assert!(live.instances["bundle0"].stopped.is_some());
        let revision = live.instances["bundle0"].presentation.output().1;
        receive(&mut live, "bundle0", next);
        assert_eq!(live.instances["bundle0"].presentation.output().1, revision);
        receive(&mut live, "bundle1", next + 1);
        assert_eq!(live.ingress.pop(u64::MAX, 1).unwrap().sequence, 2);
        assert_eq!(live.ingress.pop(u64::MAX, 1).unwrap().sequence, next + 1);
        assert!(live.ingress.pop(u64::MAX, 1).is_none());
        assert!(live.instances["bundle1"].stopped.is_none());
    }
}

#[test]
fn a_rejected_modal_stops_only_its_owner_and_restores_the_other_modal() {
    let mut live = initialized(2);
    scene(&mut live, "bundle0", 1);
    live.poll(1, CALLBACK_INTERVAL_MS, true, |_| true).unwrap();
    for (index, instance) in live.instances.values_mut().enumerate() {
        instance.opened = index as u64 + 1;
        instance.contributions.modal.open(Some(SCREEN.into()));
    }
    assert_eq!(live.modal().unwrap().bundle, "bundle1");
    assert!(live.reject_modal("invalid screen binding", CALLBACK_INTERVAL_MS));
    assert!(live.instances["bundle1"].stopped.is_some());
    assert!(live.instances["bundle0"].stopped.is_none());
    assert_eq!(live.modal().unwrap().bundle, "bundle0");
    assert_eq!(live.drawing().0.retained[0].instances.len(), 1);
    live.poll(1, 2 * CALLBACK_INTERVAL_MS, true, |_| true)
        .unwrap();
    assert!(live.instances["bundle0"].helper.is_some());
    assert!(live.reject_modal("invalid other screen", 2 * CALLBACK_INTERVAL_MS));
    assert!(live.modal().is_none());
}

/// Splits a fake helper publication so callbacks can commit a newer snapshot in flight.
fn snapshot_chunks(
    revision: u64,
    output: &mod_host::presentation::Output,
) -> VecDeque<mod_host::presentation::Chunk> {
    let bytes = serde_json::to_vec(output).unwrap();
    bytes
        .chunks(32)
        .enumerate()
        .map(|(index, part)| mod_host::presentation::Chunk {
            revision,
            offset: (index * 32) as u32,
            total: bytes.len() as u32,
            bytes: part.to_vec(),
        })
        .collect()
}

#[test]
fn mask_selected_during_transfer_is_admitted_while_culled_and_returns_to_drawing() {
    let mut live = initialized(1);
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let target = mod_api::drawing::MaskEntity {
        connection: actor.session,
        dimension: actor.dimension,
        runtime_id: actor.runtime_id,
        spawn_revision: actor.spawn_revision,
    };
    let visible = mod_host::presentation::Context {
        targets: vec![(target, 36)],
        ..context(1)
    };
    live.bind_drawing(&visible, |key| key == actor).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance
        .capabilities
        .scope
        .permissions
        .insert(Permission::EntityHighlight);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.capabilities.scope.gpu_bytes = MAX_GPU_BYTES;
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let helper = instance.helper.as_mut().unwrap();
    helper.presentation = snapshot_chunks(1, &mod_host::presentation::Output::default());
    let selected = mod_host::presentation::Output {
        masks: vec![mod_render::masks::Mask {
            handle: mod_api::drawing::DrawingHandle {
                owner: 1,
                generation: 1,
                slot: 1,
            },
            session: Default::default(),
            depth: mod_api::drawing::MaskDepth::Test,
            entities: vec![target],
            style: Some(mod_api::drawing::MaskStyle {
                fill: Some([1., 0., 0., 0.5]),
                outline: [1.; 4],
                outline_radius: 1.,
                glow: [0.; 4],
                glow_radius: 0.,
            }),
            position: None,
        }],
        ..Default::default()
    };
    live.poll(1, CALLBACK_INTERVAL_MS, true, |key| key == actor)
        .unwrap();
    assert!(live.drawing().0.masks.is_empty());
    // Select only after the older snapshot has started transferring.
    let helper = live
        .instances
        .get_mut("bundle0")
        .unwrap()
        .helper
        .as_mut()
        .unwrap();
    assert!(!helper.presentation.is_empty());
    helper.presentation.extend(snapshot_chunks(2, &selected));
    let pieces = helper.presentation.len() + 1;
    for slice in 2..=pieces {
        live.bind_drawing(&context(1), |key| key == actor).unwrap();
        live.poll(1, slice as u64 * CALLBACK_INTERVAL_MS, true, |key| {
            key == actor
        })
        .unwrap();
        let instance = &live.instances["bundle0"];
        assert!(
            instance.stopped.is_none(),
            "a live culled selection must keep running"
        );
        assert!(instance.helper.is_some());
    }
    let mut scene = render_scene::ModRenderScene::default();
    let (output, revision) = live.drawing();
    scene.apply_server(output, revision);
    assert_eq!(scene.masks[0].entities, [target]);
    assert!(scene.masks[0].authorized());
    assert_eq!(scene.masks[0].style.unwrap().fill, Some([1., 0., 0., 0.5]));
    live.bind_drawing(&visible, |key| key == actor).unwrap();
    live.poll(1, (pieces as u64 + 1) * CALLBACK_INTERVAL_MS, true, |key| {
        key == actor
    })
    .unwrap();
    let (output, revision) = live.drawing();
    scene.apply_server(output, revision);
    assert!(
        visible
            .targets
            .iter()
            .any(|(key, _)| scene.masks[0].entities.contains(key))
    );
    let instance = &live.instances["bundle0"];
    assert!(instance.stopped.is_none());
    assert!(instance.helper.as_ref().unwrap().dispatched.len() > pieces);
}

#[test]
fn callback_reference_admission_queries_liveness_outside_its_dispatch_context() {
    let actor = mod_render::retained::EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    for alive in [true, false] {
        let mut live = initialized(1);
        let instance = live.instances.get_mut("bundle0").unwrap();
        instance.dispatch(None, Event::Frame, 1, None).unwrap();
        instance.helper.as_mut().unwrap().references = vec![actor];
        live.poll(1, CALLBACK_INTERVAL_MS, false, |key| alive && key == actor)
            .unwrap();
        let instance = &live.instances["bundle0"];
        assert_eq!(instance.stopped.is_none(), alive);
        if alive {
            assert_eq!(instance.drawing_references, [actor]);
        }
    }
}
