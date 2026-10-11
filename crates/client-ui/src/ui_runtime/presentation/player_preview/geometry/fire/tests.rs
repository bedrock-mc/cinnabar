use {super::*, ui::IconRef};

fn fire(size: [f32; 2]) -> PreviewFire {
    PreviewFire {
        texture: IconRef {
            page: 7,
            uv: [16, 32, 32, 48],
            glint: false,
        },
        size,
        outer_y: 0.0,
    }
}

fn world(vertex: &UiMeshVertex) -> [f32; 3] {
    [
        (vertex.position[0] * PREVIEW_WIDTH as f32 - PREVIEW_WIDTH as f32 * 0.5)
            / PREVIEW_PIXELS_PER_BLOCK,
        (PREVIEW_FEET_Y - vertex.position[1] * PREVIEW_HEIGHT as f32) / PREVIEW_PIXELS_PER_BLOCK,
        vertex.clip_z,
    ]
}

#[test]
fn native_actor_flame_has_two_cutout_levels_at_collision_box_scale() {
    let mut vertices = Vec::new();
    let mut batches = Vec::new();
    append(&mut vertices, &mut batches, fire([0.6, 1.8])).unwrap();
    assert_eq!(vertices.len(), 24);
    let point = world(&vertices[0]);
    assert!((point[0] - 0.42).abs() < 1e-6);
    assert!(point[1].abs() < 1e-6);
    assert!((point[2] - 0.2184).abs() < 1e-6);
    assert!((world(&vertices[2])[1] - 1.176).abs() < 1e-6);
    assert!((world(&vertices[8])[1] - 1.554).abs() < 1e-6);
    assert!((vertices[6].clip_z - vertices[18].clip_z - 0.0504).abs() < 1e-6);
    assert_eq!(vertices[0].uv, [32.0, 47.95]);
    assert_eq!(vertices[2].uv, [16.0, 32.05]);
    assert!(
        vertices
            .iter()
            .all(|vertex| vertex.model_light == 1.0 && vertex.color == [255; 4])
    );
    assert_eq!(batches[0].texture_page, 7);
    assert_eq!(batches[0].index_range, 0..24);
    assert!(batches[0].depth_test && batches[0].depth_write);
    assert_eq!(batches[0].alpha_cutoff, Some(0.5));
}

#[test]
fn short_collision_boxes_limit_flame_vertical_scale_and_invalid_sizes_are_rejected() {
    let mut vertices = Vec::new();
    append(&mut vertices, &mut Vec::new(), fire([0.6, 0.4])).unwrap();
    assert!((world(&vertices[8])[1] - 1.036).abs() < 1e-6);
    for size in [[f32::NAN, 1.8], [0.0, 1.8], [0.6, -1.0]] {
        assert!(append(&mut Vec::new(), &mut Vec::new(), fire(size)).is_none());
    }
}

#[test]
fn swimming_translates_the_flames_with_the_outer_hud_frame() {
    let mut source = fire([0.6, 0.4]);
    let mut base = Vec::new();
    append(&mut base, &mut Vec::new(), source).unwrap();
    source.outer_y = super::super::super::HUD_SWIM_OFFSET;
    let mut swimming = Vec::new();
    append(&mut swimming, &mut Vec::new(), source).unwrap();
    for (base, swimming) in base.iter().zip(&swimming) {
        let base = world(base);
        let swimming = world(swimming);
        assert!((swimming[1] - base[1] - source.outer_y).abs() < 1e-6);
        assert_eq!(swimming[0], base[0]);
        assert_eq!(swimming[2], base[2]);
    }
    source.outer_y = f32::NAN;
    assert!(append(&mut Vec::new(), &mut Vec::new(), source).is_none());
}

#[test]
fn hud_fire_tints_sampled_player_geometry_and_keeps_flame_vertices_white() {
    let overlay = [0.8, 0.15, 0.0, 0.7];
    let skin = IconRef {
        page: 1,
        uv: [0, 0, 64, 64],
        glint: false,
    };
    let mesh = super::super::mesh_with_body(
        None,
        Default::default(),
        super::super::PreviewView::Hud,
        0.0,
        skin,
        &Default::default(),
        [None; 4],
        [None; 2],
        true,
        Some(fire([0.6, 1.8])),
        overlay,
    )
    .unwrap();
    let body = mesh.batches()[0].index_range.clone();
    assert!(
        mesh.vertices()[body.start as usize..body.end as usize]
            .iter()
            .all(|vertex| vertex.overlay_color == overlay)
    );
    let flame = mesh.batches().last().unwrap().index_range.clone();
    assert!(
        mesh.vertices()[flame.start as usize..flame.end as usize]
            .iter()
            .all(|vertex| vertex.overlay_color == [0.0; 4] && vertex.color == [255; 4])
    );
}
