use super::*;
use crate::actor_render::tests::allocation_fault_tests::{draw_frame, fixture, publication};

#[test]
fn reservation_shares_compatible_coverage_and_holds_charges_for_clones() {
    let app = fixture();
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>();
    let cache = world.resource::<PipelineCache>();
    let depth = test_depth(device, 64, 1);
    let bytes = mod_render::masks::attachment_bytes(64, 64, 1).unwrap() + PLACEMENT_BYTES;
    let ledger = mod_render::GpuLedger::new(bytes);
    let target = MaskTarget::new(device, cache, &depth, Some(&ledger)).unwrap();
    let alias = target.clone();
    assert_eq!(ledger.used(), bytes);
    assert!(MaskTarget::new(device, cache, &depth, Some(&ledger)).is_none());
    drop(target);
    assert_eq!(ledger.used(), bytes);
    drop(alias);
    assert_eq!(ledger.used(), 0);
    assert!(MaskTarget::new(device, cache, &depth, Some(&ledger)).is_some());
}

#[test]
fn owner_mask_reservation_combines_geometry_views_and_multisampling() {
    let mut app = fixture();
    let frame = publication(1, &[20], 1);
    let identity = frame.rig.manifest[0].identity;
    let mask = mod_render::masks::Mask {
        position: None,
        handle: DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        session: mod_api::drawing::DrawingSession {
            connection: identity.session_id,
            dimension: identity.dimension,
            local: false,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
        },
        depth: MaskDepth::Test,
        entities: vec![MaskEntity {
            connection: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision,
        }],
        style: None,
    };
    let mut output = mod_render::RenderOutput {
        masks: vec![mask.clone()].into(),
        gpu_reservations: vec![mod_render::GpuReservation {
            owners: vec![mask.handle.owner],
            ledger: mod_render::GpuLedger::new(0),
            primitives: Default::default(),
            passes: Vec::new(),
        }],
        ..Default::default()
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let view = world
        .query_filtered::<Entity, bevy::prelude::With<ExtractedView>>()
        .single(world)
        .unwrap();
    world.init_resource::<render_scene::ModRenderScene>();
    let depth = test_depth(world.resource::<RenderDevice>(), 64, 1);
    world.entity_mut(view).insert(depth);
    world
        .resource_mut::<render_scene::ModRenderScene>()
        .apply_server(&output, 1);
    draw_frame(&mut app, &frame);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .is_empty(),
        "a zero GPU reservation must allocate no mask attachments"
    );

    let bytes = mod_render::masks::attachment_bytes(64, 64, 1).unwrap() + PLACEMENT_BYTES;
    for (generation, geometry_bytes, limit_bytes, expected) in [
        (2, 1, bytes, false),
        (3, 1, bytes + 1, true),
        (4, 1, bytes, false),
    ] {
        let ledger = mod_render::GpuLedger::new(limit_bytes);
        let _geometry = ledger.allocate(geometry_bytes, || Some(())).unwrap();
        output.gpu_reservations[0].ledger = ledger;
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<render_scene::ModRenderScene>()
            .apply_server(&output, generation);
        draw_frame(&mut app, &frame);
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        draw_frame(&mut app, &frame);
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<MaskGpu>()
                .targets
                .contains_key(&(view, mask.handle)),
            expected
        );
    }

    output.gpu_reservations[0].ledger = mod_render::GpuLedger::new(bytes);
    let world = app.sub_app_mut(RenderApp).world_mut();
    world
        .resource_mut::<render_scene::ModRenderScene>()
        .apply_server(&output, 5);
    let depth = test_depth(world.resource::<RenderDevice>(), 64, 4);
    world.entity_mut(view).insert((depth, Msaa::Sample4));
    draw_frame(&mut app, &frame);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .is_empty()
    );

    let world = app.sub_app_mut(RenderApp).world_mut();
    let depth = test_depth(world.resource::<RenderDevice>(), 128, 1);
    world.entity_mut(view).insert((depth, Msaa::Off));
    draw_frame(&mut app, &frame);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .is_empty()
    );

    let world = app.sub_app_mut(RenderApp).world_mut();
    let depth = test_depth(world.resource::<RenderDevice>(), 64, 1);
    world.entity_mut(view).insert(depth);
    let other_depth = test_depth(world.resource::<RenderDevice>(), 64, 1);
    let other = world.spawn((Msaa::Off, other_depth)).id();
    draw_frame(&mut app, &frame);
    assert_eq!(
        output.gpu_reservations[0].ledger.used(),
        bytes,
        "each view uses the same ledger"
    );
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .len()
            <= 1
    );
    app.sub_app_mut(RenderApp).world_mut().despawn(other);
    draw_frame(&mut app, &frame);
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .len(),
        1
    );
}
