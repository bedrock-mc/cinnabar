use super::*;

#[test]
fn draw_lists_keep_their_camera_sorted_order() {
    let mut frame = ParticleGpuFrame::default();
    let blend = vec![
        ParticleInstance {
            center_light: [4.0, 8.0, -10.0, 0.0],
            ..Default::default()
        },
        ParticleInstance {
            center_light: [2.0, 4.0, -6.0, 0.0],
            ..Default::default()
        },
    ];
    frame.set_lists(DrawLists {
        opaque: vec![ParticleInstance {
            center_light: [100.0, 0.0, -1.0, 0.0],
            ..Default::default()
        }],
        blend: blend.clone(),
        cave_hidden: 3,
        add: vec![ParticleInstance {
            center_light: [1.0, 3.0, -5.0, 0.0],
            ..Default::default()
        }],
        ..DrawLists::default()
    });
    assert_eq!(frame.blend.as_ref(), blend.as_slice());
    assert_eq!(
        (frame.opaque.len(), frame.blend.len(), frame.add.len()),
        (1, 2, 1)
    );
    assert_eq!(frame.instance_count(), 4);
    assert_eq!(frame.cave_hidden, 3);
    frame.set_lists(DrawLists::default());
    assert_eq!(frame.instance_count(), 0);
    assert_eq!(frame.cave_hidden, 0);
}

#[test]
fn unchanged_draw_lists_keep_their_publication() {
    let mut frame = ParticleGpuFrame::default();
    let lists = || DrawLists {
        opaque: vec![ParticleInstance::default()],
        cave_hidden: 1,
        ..DrawLists::default()
    };
    assert!(frame.set_lists(lists()), "the first real lists publish");
    let (opaque, blend) = (Arc::clone(&frame.opaque), Arc::clone(&frame.blend));
    assert!(
        !frame.set_lists(lists()),
        "identical lists are not republished"
    );
    assert!(Arc::ptr_eq(&opaque, &frame.opaque));
    assert!(Arc::ptr_eq(&blend, &frame.blend));

    assert!(
        frame.set_lists(DrawLists {
            cave_hidden: 2,
            ..lists()
        }),
        "a hidden-count change alone publishes"
    );
    assert!(Arc::ptr_eq(&opaque, &frame.opaque));
    let mut signed_zero = lists();
    signed_zero.opaque[0].center_light[0] = -0.0;
    assert!(
        frame.set_lists(signed_zero),
        "a sign-only change still publishes"
    );
    assert_eq!(
        frame.opaque[0].center_light[0].to_bits(),
        (-0.0f32).to_bits()
    );
    assert!(frame.set_lists(DrawLists::default()));
    assert_eq!(frame.instance_count(), 0);
}
