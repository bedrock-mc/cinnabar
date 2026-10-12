use super::*;

#[test]
fn draw_lists_keep_separate_centroids_and_use_the_camera_when_empty() {
    let camera = [7.0, 8.0, 9.0];
    let mut frame = ParticleGpuFrame::default();
    frame.set_lists(
        DrawLists {
            opaque: vec![ParticleInstance {
                center_light: [100.0, 0.0, -1.0, 0.0],
                ..Default::default()
            }],
            blend: vec![
                ParticleInstance {
                    center_light: [2.0, 4.0, -6.0, 0.0],
                    ..Default::default()
                },
                ParticleInstance {
                    center_light: [4.0, 8.0, -10.0, 0.0],
                    ..Default::default()
                },
            ],
            cave_hidden: 3,
            add: vec![ParticleInstance {
                center_light: [1.0, 3.0, -5.0, 0.0],
                ..Default::default()
            }],
        },
        camera,
    );
    assert_eq!(frame.blend_centroid, [3.0, 6.0, -8.0]);
    assert_eq!(frame.add_centroid, [1.0, 3.0, -5.0]);
    assert_eq!(
        (frame.opaque.len(), frame.blend.len(), frame.add.len()),
        (1, 2, 1)
    );
    assert_eq!(frame.instance_count(), 4);
    assert_eq!(frame.cave_hidden, 3);
    frame.set_lists(DrawLists::default(), camera);
    assert_eq!(frame.blend_centroid, camera);
    assert_eq!(frame.add_centroid, camera);
    assert_eq!(frame.instance_count(), 0);
    assert_eq!(frame.cave_hidden, 0);
}
