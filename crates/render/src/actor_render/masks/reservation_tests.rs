use super::*;
use crate::actor_render::tests::allocation_fault_tests::{draw_frame, fixture, publication};

#[test]
fn reservation_shares_compatible_coverage_and_halos_across_its_owners() {
    let first = mod_render::masks::Mask {
        handle: DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        },
        position: None,
        session: mod_api::drawing::DrawingSession::default(),
        depth: MaskDepth::Test,
        entities: Vec::new(),
        style: Some(mod_api::drawing::MaskStyle {
            fill: None,
            outline: [0.; 4],
            outline_radius: 0.,
            glow: [1.; 4],
            glow_radius: 4.,
        }),
    };
    let mut second = first.clone();
    second.handle.owner = 2;
    let masks = [first, second];
    let bytes = mod_render::masks::attachment_bytes(63, 65, 4).unwrap();
    let halo = mod_render::masks::halo_bytes(63, 65).unwrap();
    let mut reservation = mod_render::GpuReservation {
        owners: vec![1, 2],
        geometry_bytes: 17,
        limit_bytes: bytes + halo + 17,
    };
    let mut active = masks.iter().collect();
    admit_reservations(&mut active, &[reservation.clone()], Some((bytes, halo)));
    assert_eq!(
        active.len(),
        2,
        "identical masks share one coverage and halo group"
    );
    reservation.limit_bytes -= 1;
    admit_reservations(&mut active, &[reservation.clone()], Some((bytes, halo)));
    assert!(
        active.is_empty(),
        "halo attachments share the geometry allowance"
    );

    let mut active = masks.iter().collect();
    reservation.owners = vec![1];
    reservation.limit_bytes = 0;
    admit_reservations(&mut active, &[reservation], Some((bytes, halo)));
    assert_eq!(
        active.len(),
        1,
        "another owner's compatible target grants no allowance"
    );
    assert_eq!(active[0].handle.owner, 2);
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
            geometry_bytes: 0,
            limit_bytes: 0,
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

    let bytes = mod_render::masks::attachment_bytes(64, 64, 1).unwrap();
    for (generation, geometry_bytes, limit_bytes, expected) in [
        (2, 1, bytes, false),
        (3, 1, bytes + 1, true),
        (4, 1, bytes, false),
    ] {
        output.gpu_reservations[0].geometry_bytes = geometry_bytes;
        output.gpu_reservations[0].limit_bytes = limit_bytes;
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<render_scene::ModRenderScene>()
            .apply_server(&output, generation);
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

    output.gpu_reservations[0].geometry_bytes = 0;
    output.gpu_reservations[0].limit_bytes = bytes;
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
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<MaskGpu>()
            .targets
            .is_empty(),
        "all views share the owner's reservation"
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
