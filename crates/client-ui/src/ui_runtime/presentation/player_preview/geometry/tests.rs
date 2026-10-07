use super::*;

fn skin(side: u16) -> IconRef {
    IconRef {
        page: 23,
        uv: [17, 29, 17 + side, 29 + side],
        glint: false,
    }
}

fn bare(view: PreviewView, side: u16) -> Arc<UiMesh> {
    mesh(
        Default::default(),
        view,
        0.0,
        skin(side),
        &Default::default(),
        [None; 4],
        [None; 2],
        false,
    )
    .expect("valid standard-biped mesh")
}

#[test]
fn destination_geometry_preserves_original_hd_skin_texel_edges() {
    let source = standard_biped_vertices();
    let model = bare(Default::default(), 128);
    assert_eq!(model.vertices().len(), source.len() * 2);
    for (vertex, original) in model.vertices().iter().zip(&source) {
        assert_eq!(
            vertex.uv,
            [17.0 + original.uv[0] * 128.0, 29.0 + original.uv[1] * 128.0,]
        );
        assert_eq!(vertex.color, [255; 4]);
        assert_eq!(vertex.model_light, 1.0);
        assert_eq!(vertex.clip_w, 1.0);
        assert!((0.25..=0.75).contains(&vertex.clip_z));
    }
    let batch = &model.batches()[0];
    assert_eq!(batch.texture_page, skin(128).page);
    assert_eq!(batch.alpha_cutoff, Some(0.5));
    assert!(batch.depth_test && batch.depth_write);
    assert_eq!(model.indices().len(), model.vertices().len());
}

#[test]
fn mesh_uses_continuous_geometry_not_cpu_preview_pixel_centres() {
    let model = bare(
        PreviewView::Live {
            offset: [13.25, -7.5],
        },
        64,
    );
    assert!(model.vertices().iter().any(|vertex| {
        let screen_x = vertex.position[0] * PREVIEW_WIDTH as f32;
        (screen_x - screen_x.round()).abs() > 0.05
    }));
    // Texture resolution changes only UV edges, never silhouette positions.
    let hd = bare(
        PreviewView::Live {
            offset: [13.25, -7.5],
        },
        256,
    );
    for (low, high) in model.vertices().iter().zip(hd.vertices()) {
        assert_eq!(low.position, high.position);
        assert_eq!(low.clip_z, high.clip_z);
    }
}

#[test]
fn native_live_angles_remain_separate_body_head_and_model_pitch() {
    let angles = PreviewView::Live {
        offset: [40.0, 40.0],
    }
    .angles();
    let quarter = std::f32::consts::FRAC_PI_4;
    assert_eq!(
        angles,
        [
            quarter * 20.0,
            quarter * 40.0,
            quarter * -20.0,
            quarter * -20.0
        ]
    );
}

#[test]
fn hud_faces_the_players_left_and_keeps_body_facing_fixed() {
    let source = standard_biped_vertices();
    let model = bare(PreviewView::Hud, 64);
    let mean_x = |front: bool| {
        let positions: Vec<_> = source
            .iter()
            .zip(model.vertices())
            .filter(|(source, _)| source.part == 1 && (source.position[2] > 0.0) == front)
            .map(|(_, projected)| projected.position[0])
            .collect();
        positions.iter().sum::<f32>() / positions.len() as f32
    };
    assert!(
        mean_x(true) > mean_x(false),
        "native front faces screen-right"
    );
    for yaw in [-135.0, 0.0, 75.0, 180.0] {
        let turned = mesh(
            PlayerPreviewPose::new(yaw, yaw, 0.0, false),
            PreviewView::Hud,
            0.0,
            skin(64),
            &Default::default(),
            [None; 4],
            [None; 2],
            false,
        )
        .unwrap();
        for (rest, turned) in model.vertices().iter().zip(turned.vertices()) {
            assert_eq!(
                rest.position, turned.position,
                "world yaw must not turn the HUD body"
            );
        }
    }
}

#[test]
fn hud_keeps_evaluated_head_motion_without_turning_the_body() {
    let source = standard_biped_vertices();
    let posed = |head_yaw: f32| {
        source
            .iter()
            .map(|vertex| {
                let mut vertex = *vertex;
                if vertex.part == 0 {
                    vertex.position = super::super::rotate_y(
                        vertex.position,
                        head_yaw.to_radians(),
                        [0.0, 1.5, 0.0],
                    );
                }
                // The HUD capture has already posed these vertices, so it places them
                // outside the six equipment transforms to prevent posing twice.
                vertex.part += 6;
                vertex
            })
            .collect::<Vec<_>>()
    };
    let project = |vertices: &[ActorVertex]| {
        mesh_with_body(
            Some((vertices, &[None; 6])),
            Default::default(),
            PreviewView::Hud,
            0.0,
            skin(64),
            &Default::default(),
            [None; 4],
            [None; 2],
            false,
            None,
            [0.0; 4],
        )
        .unwrap()
    };
    let rest = project(&posed(0.0));
    let looking = project(&posed(60.0));
    let mut changed_head = false;
    for ((source, rest), looking) in source.iter().zip(rest.vertices()).zip(looking.vertices()) {
        if source.part == 0 {
            changed_head |= rest.position != looking.position;
        } else {
            assert_eq!(
                rest.position, looking.position,
                "head look must not turn the body"
            );
        }
    }
    assert!(changed_head, "evaluated head look survives HUD projection");
}

#[test]
fn equipment_batches_keep_source_pages_tint_and_shared_model_depth() {
    let armor_texture = super::super::PreviewTexture {
        rgba: Arc::from(vec![255; 64 * 32 * 4]),
        width: 64,
        height: 32,
        tint: Some([110, 70, 200]),
    };
    let gear = PreviewEquipment {
        armor: [Some(armor_texture), None, None, None],
        held: None,
        ..Default::default()
    };
    let armor = IconRef {
        page: 24,
        uv: [10, 20, 74, 52],
        glint: false,
    };
    let held = IconRef {
        page: 25,
        uv: [5, 7, 21, 23],
        glint: true,
    };
    let held_model = super::super::PreviewHeldModel {
        source: held,
        vertices: render_model::held_sprite_vertices(
            16,
            16,
            &[255; 16 * 16 * 4],
            [0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
        .into(),
        placements: [super::super::PreviewHeldPlacement::Sprite {
            hand_equipped: false,
        }; 2],
        hand_pivots: [
            [6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
            [-6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
        ],
    };
    let model = mesh(
        Default::default(),
        Default::default(),
        0.0,
        skin(64),
        &gear,
        [Some(armor), None, None, None],
        [Some(&held_model), None],
        false,
    )
    .expect("skin, armor and held mesh");
    assert_eq!(model.batches().len(), 3);
    let armor_range = &model.batches()[1].index_range;
    assert_eq!(model.batches()[1].texture_page, armor.page);
    assert_eq!(model.batches()[1].alpha_cutoff, Some(1.0 / 255.0));
    assert!(
        model.vertices()[armor_range.start as usize..armor_range.end as usize]
            .iter()
            .all(|vertex| vertex.color == [110, 70, 200, 255]
                && u32::from(vertex.style_flags) & render_model::UI_STYLE_COLOR_MASK != 0)
    );
    let held_range = &model.batches()[2].index_range;
    assert_eq!(model.batches()[2].texture_page, held.page);
    assert!(
        model.vertices()[held_range.start as usize..held_range.end as usize]
            .iter()
            .all(|vertex| vertex.style_flags == UI_STYLE_GLINT)
    );
    assert!(
        model
            .vertices()
            .iter()
            .all(|vertex| vertex.clip_z > 0.0 && vertex.clip_z < 1.0)
    );
}

#[test]
fn invalid_texture_region_and_nonfinite_view_do_not_publish_geometry() {
    let invalid = IconRef {
        uv: [0; 4],
        ..skin(64)
    };
    assert!(
        mesh(
            Default::default(),
            Default::default(),
            0.0,
            invalid,
            &Default::default(),
            [None; 4],
            [None; 2],
            false,
        )
        .is_none()
    );
    assert!(
        mesh(
            Default::default(),
            PreviewView::Doll {
                yaw: f32::NAN,
                tilt: 0.0
            },
            0.0,
            skin(64),
            &Default::default(),
            [None; 4],
            [None; 2],
            false,
        )
        .is_none()
    );
}

#[test]
fn fancy_entity_material_keeps_float_directional_lighting_separate_from_tint() {
    let model = mesh(
        Default::default(),
        Default::default(),
        0.0,
        skin(64),
        &Default::default(),
        [None; 4],
        [None; 2],
        true,
    )
    .expect("fancy player model");
    // Each source cuboid has six faces, in east/front/west/back/top/bottom order.
    for (face, expected) in model.vertices()[..36]
        .chunks_exact(6)
        .zip([0.625, 0.825, 0.625, 0.825, 1.0, 0.45])
    {
        for vertex in face {
            assert_eq!(vertex.color, [255; 4]);
            assert!((vertex.model_light - expected).abs() < 1e-6);
        }
    }
    let rotated = mesh(
        Default::default(),
        PreviewView::Doll {
            yaw: 12.3,
            tilt: 14.7,
        },
        0.0,
        skin(64),
        &Default::default(),
        [None; 4],
        [None; 2],
        true,
    )
    .expect("tilted fancy model");
    assert!(
        rotated
            .vertices()
            .iter()
            .any(|vertex| { (vertex.model_light * 255.0).fract().abs() > 0.01 })
    );
}

#[test]
fn native_pose_and_bob_are_not_quantized_for_gpu_geometry() {
    let pose = PlayerPreviewPose::new(11.123, -32.71, 6.111, false);
    assert_eq!(pose.body_yaw_degrees, 11.123);
    assert_eq!(pose.head_yaw_degrees, -32.71);
    assert_eq!(pose.pitch_degrees, 6.111);
    let bob = super::super::bob_degrees(0.317);
    let native = ((0.317f64 * 103.2).to_radians().cos() * 2.865 + 2.865) as f32;
    assert_eq!(bob, native);
    assert!((bob * 2.0 - (bob * 2.0).round()).abs() > 0.01);
}

#[test]
fn fractional_texture_edges_are_preserved_not_rounded() {
    let model = bare(Default::default(), 65);
    assert!(
        model
            .vertices()
            .iter()
            .any(|vertex| { vertex.uv.iter().any(|edge| edge.fract() != 0.0) })
    );
    assert_eq!(atlas_edge(10, 26, 0.5 / 16.0), Some(10.5));
    assert_eq!(atlas_edge(10, 26, f32::NAN), None);
    assert_eq!(atlas_edge(10, 26, -0.1), None);
}

#[test]
fn paperdoll_controller_does_not_apply_live_bob_sneak_or_holding_rotation() {
    let view = PreviewView::Doll {
        yaw: 17.0,
        tilt: 12.0,
    };
    let stationary = Rig::new(Default::default(), view, 0.0, [false; 2]);
    let crouched_held = Rig::new(
        PlayerPreviewPose::new(0.0, 0.0, 0.0, true),
        view,
        5.2,
        [true; 2],
    );
    for vertex in standard_biped_vertices() {
        assert_eq!(
            stationary.project(vertex).world,
            crouched_held.project(vertex).world
        );
    }
}
