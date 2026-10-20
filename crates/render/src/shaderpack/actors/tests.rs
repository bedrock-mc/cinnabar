use super::classify::{
    DrawValues, ENTITY_GLINT_SCALE, classify, entity_colour, glint_texture_matrix, hand_projection,
    vertex_colour,
};
use super::frame::{MAX_WORKGROUPS, linear_workgroups, rig_workgroups};
use super::skin::SKIN_WORKGROUP_SIZE;
use crate::shaderpack::draws::DrawKind;
use crate::shaderpack::uniforms::patch_block;
use assets::EntityRenderMaterial;
use assets::EntityRenderMaterialState as State;
use bevy::math::{Mat4, Vec3, Vec4};

fn authored(flags: u32) -> u32 {
    State::AUTHORED | flags
}

#[test]
fn actors_draw_in_the_iris_program_their_material_selects() {
    let entities = classify(0, 0.0, None, false).unwrap();
    assert_eq!(entities.kind, DrawKind::Entities);
    assert!(entities.shadow && !entities.glint && !entities.one_sided);
    let blended = classify(authored(State::BLEND), 0.0, None, false).unwrap();
    assert_eq!(blended.kind, DrawKind::EntitiesTranslucent);
    assert!(blended.shadow);
    let eyes = classify(authored(State::BLEND | State::ADDITIVE), 0.0, None, false).unwrap();
    assert_eq!(eyes.kind, DrawKind::SpiderEyes);
    assert!(!eyes.shadow, "additive eye layers cast no shadow");
    let culled = classify(authored(State::CULL | State::ALPHA_TEST), 0.0, None, false).unwrap();
    assert!(culled.one_sided);
    assert_eq!(
        classify(EntityRenderMaterial::DissolveDepth as u32, 0.0, None, false),
        None,
        "dissolve's depth-only helper is not drawn"
    );
    let glint = classify(EntityRenderMaterial::Glint as u32, 0.0, None, false).unwrap();
    assert!(glint.glint);
    assert!(
        classify(0, 0.5, None, false).unwrap().glint,
        "item glint extent"
    );
}

#[test]
fn first_person_instances_draw_as_the_hand() {
    let hand = classify(0, 0.0, Some(false), true).unwrap();
    assert_eq!(hand.kind, DrawKind::Hand);
    assert!(!hand.shadow);
    assert_eq!(
        classify(0, 0.0, Some(true), true).unwrap().kind,
        DrawKind::HandWater
    );
}

#[test]
fn dye_becomes_the_vertex_colour_and_overlay_the_entity_colour() {
    assert_eq!(vertex_colour(0, false), u32::MAX, "no dye is white");
    let dye = u32::from_le_bytes([200, 100, 50, 128]);
    assert_eq!(vertex_colour(dye, false).to_le_bytes(), [200, 100, 50, 255]);
    assert_eq!(vertex_colour(dye, true).to_le_bytes(), [200, 100, 50, 128]);
    let hurt = render_model::pack_overlay_rgba8([1.0, 0.0, 0.0, 0.6]);
    let colour = entity_colour(hurt, 0);
    assert!((colour - Vec4::new(1.0, 0.0, 0.0, 0.6)).abs().max_element() < 0.01);
    assert_eq!(
        entity_colour(hurt, authored(State::DISABLE_OVERLAY)),
        Vec4::ZERO
    );
}

#[test]
fn per_draw_values_patch_iris_entity_uniforms() {
    use shaderpack::{ScalarType, UniformBlock, UniformMember, ValueType};
    let member = |name: &str, offset, ty, array| UniformMember {
        name: name.into(),
        offset,
        ty,
        array,
    };
    let mat4 = ValueType::Matrix {
        columns: 4,
        rows: 4,
        scalar: ScalarType::Float,
    };
    let block = UniformBlock {
        name: "uniforms".into(),
        group: 0,
        binding: 0,
        stages: Default::default(),
        size: 208,
        members: vec![
            member("entityId", 0, ValueType::Scalar(ScalarType::Int), None),
            member(
                "entityColor",
                16,
                ValueType::Vector {
                    size: 4,
                    scalar: ScalarType::Float,
                },
                None,
            ),
            member("iris_TextureMat", 32, mat4, Some((2, 64))),
            member(
                "blockEntityId",
                160,
                ValueType::Scalar(ScalarType::Int),
                None,
            ),
        ],
    };
    let mut bytes = vec![0xffu8; 208];
    let values = DrawValues {
        entity_id: 10_102,
        entity_color: Vec4::new(1.0, 0.0, 0.0, 0.3),
        texture_matrix: Mat4::from_translation(Vec3::new(0.5, 0.25, 0.0)),
        ..DrawValues::NONE
    };
    patch_block(&block, &values.uniforms(), &mut bytes);
    let int = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(int(0), 10_102);
    assert_eq!(float(16), 1.0);
    assert!((float(28) - 0.3).abs() < 1e-6);
    assert_eq!(float(32 + 48), 0.5, "element 0's translation column");
    assert_eq!(float(32 + 52), 0.25);
    assert_eq!(
        &bytes[96..160],
        &[0xff; 64],
        "element 1 keeps the lightmap matrix"
    );
    assert_eq!(int(160), -1);
}

#[test]
fn the_glint_matrix_uses_configured_scrolls_with_javas_rotation_and_scale() {
    let start = glint_texture_matrix(
        render_model::ActorGlint::default().parameters(),
        ENTITY_GLINT_SCALE,
    );
    let expected = Mat4::from_rotation_z(10f32.to_radians())
        * Mat4::from_scale(Vec3::splat(ENTITY_GLINT_SCALE));
    assert!(start.abs_diff_eq(expected, 1e-6));
    for speed in [0.0, 0.5, 1.0] {
        let glint = render_model::ActorGlint {
            speed,
            ..render_model::ActorGlint::item(2.5, [0.25, 0.125])
        };
        let later = glint_texture_matrix(glint.parameters(), 1.0);
        let scroll = glint.scroll();
        assert!(
            later
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(scroll[0], scroll[1], 0.0), 1e-6)
        );
    }
}

#[test]
fn the_hand_projection_keeps_hand_depths_around_one_half() {
    let projection = hand_projection(70f32.to_radians(), [1600, 900], 256.0);
    for distance in [0.1f32, 0.5, 2.0] {
        let clip = projection * Vec4::new(0.2, -0.3, -distance, 1.0);
        let window = (clip.z / clip.w) * 0.5 + 0.5;
        assert!((0.4375..=0.5625).contains(&window), "{distance} → {window}");
    }
    let unscaled = super::super::uniforms::gl_perspective(
        [
            1.0 / (35f32.to_radians().tan() * 16.0 / 9.0),
            1.0 / 35f32.to_radians().tan(),
        ],
        super::super::uniforms::GL_NEAR,
        256.0,
    );
    let point = Vec4::new(0.2, -0.3, -0.5, 1.0);
    let (a, b) = (projection * point, unscaled * point);
    assert!((a.z - b.z * shaderpack::Environment::default().hand_depth).abs() < 1e-6);
    assert!(
        (a.x / a.w - b.x / b.w).abs() < 1e-5,
        "screen position unchanged"
    );
}

#[test]
fn dispatches_cover_every_job_and_vertex() {
    assert_eq!(rig_workgroups(3, 130), [3, 3, 1]);
    assert_eq!(rig_workgroups(70_000, 64), [1, MAX_WORKGROUPS, 2]);
    assert_eq!(linear_workgroups(1), [1, 1, 1]);
    let big = linear_workgroups(MAX_WORKGROUPS * SKIN_WORKGROUP_SIZE + 1);
    assert_eq!(big, [MAX_WORKGROUPS, 2, 1]);
}

#[test]
fn glint_keeps_base_coverage_and_first_person_matrices() {
    for kind in [
        DrawKind::ArmorGlint,
        DrawKind::ArmorGlintTranslucent,
        DrawKind::HandGlint,
    ] {
        assert_eq!(kind.depth_compare(), wgpu::CompareFunction::Equal);
        assert!(!kind.depth_write(&Default::default()));
    }
    assert!(DrawKind::HandGlint.hand() && DrawKind::HandGlint.after_deferred());
    assert!(DrawKind::ArmorGlintTranslucent.after_deferred());
}

#[test]
fn translucent_actor_order_follows_the_view_and_keeps_draw_slots() {
    use super::view::{ActorDraw, ViewActors};
    let mut actors = ViewActors::default();
    let texture = bevy::render::render_resource::TextureId::new();
    for distance in [2., 12., 5.] {
        actors.draws.push(ActorDraw {
            kind: DrawKind::EntitiesTranslucent,
            vertices: 0..6,
            texture: (texture, 0),
            values: DrawValues::NONE,
            distance,
        });
    }
    actors.slot_of = vec![10, 20, 30];
    let ordered: Vec<_> = actors
        .draw_order(DrawKind::EntitiesTranslucent)
        .iter()
        .map(|i| actors.slot_of[*i])
        .collect();
    assert_eq!(ordered, [20, 30, 10]);
    for draw in &mut actors.draws {
        draw.distance = -draw.distance;
    }
    let ordered: Vec<_> = actors
        .draw_order(DrawKind::EntitiesTranslucent)
        .iter()
        .map(|i| actors.slot_of[*i])
        .collect();
    assert_eq!(ordered, [10, 30, 20]);
}

#[test]
fn actor_vertex_inputs_bind_full_float_uvs() {
    use shaderpack::{ScalarType, ValueType, VertexAttribute, VertexInput};
    let inputs = [VertexAttribute::TexCoord, VertexAttribute::MidTexCoord]
        .into_iter()
        .enumerate()
        .map(|(location, attribute)| VertexInput {
            location: location as u32,
            name: format!("uv{location}"),
            attribute,
            ty: ValueType::Vector {
                size: 2,
                scalar: ScalarType::Float,
            },
        })
        .collect::<Vec<_>>();
    let layouts = super::layout::vertex_layouts(&inputs).unwrap();
    let stream = &layouts[crate::shaderpack::geometry::GEOMETRY_VERTEX_BUFFER as usize];
    assert_eq!(stream.array_stride, super::layout::ACTOR_VERTEX_STRIDE);
    assert!(
        stream
            .attributes
            .iter()
            .all(|attribute| attribute.format == wgpu::VertexFormat::Float32x2)
    );
}

#[test]
fn hand_translucent_depth_is_independent_of_the_world_camera() {
    let position = Vec3::new(1., 0., -2.);
    for forward in [Vec3::X, Vec3::NEG_Z] {
        assert_eq!(
            super::view::draw_depth(position, Vec3::splat(10_000.), forward, true),
            2.
        );
    }
    assert_eq!(
        super::view::draw_depth(position, Vec3::ZERO, Vec3::NEG_Z, false),
        2.
    );
    assert_eq!(
        super::view::draw_depth(position, Vec3::ZERO, Vec3::Z, false),
        -2.
    );
}
