use super::*;
use mod_api::drawing::{DrawingHandle, MaskEntity};
use mod_render::retained::{Anchor, EntityAnchor, EntityKey};

#[test]
fn despawn_during_callback_drops_only_stale_drawing_and_keeps_the_bundle_running() {
    let mut live = initialized(1);
    let stale = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let alive = EntityKey {
        runtime_id: 43,
        ..stale
    };
    let replacement = EntityKey {
        spawn_revision: 4,
        ..stale
    };
    let mut observed = context(1);
    observed.candidates = vec![stale, alive];
    observed.live = observed.candidates.clone();
    live.bind_drawing(&observed, |_| true).unwrap();
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.capabilities.scope.permissions.extend([
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::EntityHighlight,
    ]);
    instance.capabilities.scope.memory_bytes = MAX_GUEST_MEMORY;
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let mut output = retained_scene(1);
    for (slot, key) in [stale, alive].into_iter().enumerate() {
        let entity = DrawingHandle {
            owner: 99,
            generation: 1,
            slot: slot as u64,
        };
        output.drawing.entities.push((entity, key));
        let mut draw = output.drawing.instances[0].1.clone();
        draw.anchor = Anchor::Entity(EntityAnchor {
            entity,
            body_rotation: true,
        });
        output.drawing.instances.push((
            DrawingHandle {
                slot: 10 + slot as u64,
                ..entity
            },
            draw,
        ));
    }
    output.masks.push(mod_render::masks::Mask {
        handle: DrawingHandle {
            owner: 99,
            generation: 1,
            slot: 20,
        },
        session: Default::default(),
        depth: mod_api::drawing::MaskDepth::Test,
        entities: [stale, alive]
            .map(|key| MaskEntity {
                connection: key.session,
                dimension: key.dimension,
                runtime_id: key.runtime_id,
                spawn_revision: key.spawn_revision,
            })
            .to_vec(),
        style: None,
        position: None,
    });
    output.time = Some(1234);
    let helper = instance.helper.as_mut().unwrap();
    helper.references = vec![stale, alive];
    helper.presentation = snapshot_chunks(1, &output);
    let pieces = helper.presentation.len();
    // The first actor despawned and its runtime ID was reused while the callback ran.
    let valid = |key| key == alive || key == replacement;
    live.bind_drawing(&context(1), valid).unwrap();
    for slice in 1..=pieces {
        live.poll(1, slice as u64 * CALLBACK_INTERVAL_MS, true, valid)
            .unwrap();
        let instance = &live.instances["bundle0"];
        assert!(instance.stopped.is_none());
        assert!(instance.helper.is_some());
        assert_eq!(instance.drawing_references, [alive]);
        assert!(instance.strikes.is_empty());
    }
    let instance = &live.instances["bundle0"];
    let output = instance.presentation.output().0;
    let resources = &output.retained[0];
    assert_eq!(
        resources.entities.values().copied().collect::<Vec<_>>(),
        [alive]
    );
    assert_eq!(resources.instances.len(), 2);
    assert_eq!(output.masks[0].entities.len(), 1);
    assert_eq!(
        output.masks[0].entities[0].spawn_revision,
        alive.spawn_revision
    );
    assert_eq!(instance.presentation.time, Some(1234));
    assert_eq!(instance.presentation.stale_references, pieces as u64 + 2);
    assert!(instance.helper.as_ref().unwrap().dispatched.len() > pieces);
}

#[test]
fn failed_callback_drops_despawned_references_without_a_transport_fault() {
    let mut live = initialized(1);
    let actor = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 3,
    };
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.dispatch(None, Event::Frame, 1, None).unwrap();
    let helper = instance.helper.as_mut().unwrap();
    helper.references = vec![actor];
    helper.response = None;
    helper.failure = Some(CallFailure {
        bundle: "bundle0".into(),
        callback: "frame".into(),
        kind: FailureKind::Panic,
        reason: "fixture trap".into(),
        fuel: Some(1),
    });
    live.poll(1, CALLBACK_INTERVAL_MS, false, |_| false)
        .unwrap();
    let instance = &live.instances["bundle0"];
    assert!(instance.stopped.is_none());
    assert!(instance.helper.is_some());
    assert!(instance.drawing_references.is_empty());
    assert_eq!(instance.strikes.len(), 1);
    assert_eq!(instance.presentation.stale_references, 1);
}
