use super::*;
#[path = "local_light_texture_tests.rs"]
mod textures;

fn inputs() -> PreparedInputs {
    PreparedInputs {
        lights_revision: 0,
        dimension: Some(0),
        camera: Vec3::ZERO,
        clip: Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.05, 96.0),
        size: [256, 256],
        first_layer: 0,
    }
}

fn source(position: Vec3, dimension: i32) -> LightSource {
    LightSource {
        position,
        level: 15,
        dimension,
    }
}

#[test]
fn selection_filters_dimension_and_behind_camera_and_reuses_bounded_capacity() {
    let mut sources = LocalLightSources::default();
    sources.chunks.insert(
        Entity::from_bits(1),
        (0..80)
            .map(|index| source(Vec3::new(index as f32 * 0.05, 0.0, -24.0), 0))
            .collect(),
    );
    sources.chunks.insert(
        Entity::from_bits(2),
        vec![source(Vec3::new(0.0, 0.0, 24.0), 0)],
    );
    sources.chunks.insert(
        Entity::from_bits(3),
        vec![source(Vec3::new(0.0, 0.0, -4.0), 1)],
    );
    let mut candidates = Vec::with_capacity(MAX_LOCAL_LIGHTS);
    selection::select(&mut candidates, &sources, &inputs());
    assert_eq!(candidates.len(), MAX_LOCAL_LIGHTS);
    assert!(
        candidates
            .iter()
            .all(|light| light.dimension == 0 && light.position.z < 0.0)
    );
    let capacity = candidates.capacity();
    let pointer = candidates.as_ptr();
    let selected = candidates.clone();
    selection::select(&mut candidates, &sources, &inputs());
    assert_eq!(candidates, selected);
    assert_eq!(candidates.capacity(), capacity);
    assert_eq!(candidates.as_ptr(), pointer);
    let mut unknown = inputs();
    unknown.dimension = None;
    selection::select(&mut candidates, &sources, &unknown);
    assert!(
        candidates.is_empty(),
        "unknown view dimension admits no cross-world lights"
    );
}

#[test]
fn tiles_choose_visible_contributors_instead_of_camera_order_and_reuse_storage() {
    let input = inputs();
    let mut candidates = vec![source(Vec3::new(-10.0, 0.0, -14.0), 0); TILE_LIGHTS];
    candidates.push(source(Vec3::new(8.0, 0.0, -24.0), 0));
    let (mut words, mut scores, mut rays) = (Vec::new(), Vec::new(), Vec::new());
    selection::tiles(&candidates, 0, &input, &mut words, &mut scores, &mut rays);
    let right_tile = 4 + (3 * words[0] + 6) as usize * (TILE_LIGHTS + 1);
    let count = words[right_tile] as usize;
    assert!(
        words[right_tile + 1..right_tile + 1 + count].contains(&(TILE_LIGHTS as u32)),
        "a bright contributor to the tile must survive global camera ordering"
    );
    assert!(
        words[4..]
            .chunks_exact(TILE_LIGHTS + 1)
            .all(|tile| tile[0] as usize <= TILE_LIGHTS)
    );
    let pointers = (words.as_ptr(), scores.as_ptr(), rays.as_ptr());
    let reference = words.clone();
    selection::tiles(&candidates, 0, &input, &mut words, &mut scores, &mut rays);
    assert_eq!(words, reference);
    assert_eq!(pointers, (words.as_ptr(), scores.as_ptr(), rays.as_ptr()));
}

#[test]
fn off_axis_light_sphere_bounds_are_conservative() {
    let input = inputs();
    let center = Vec3::new(24.0, 0.0, -30.0);
    let (low, high) = selection::tile_bounds(center, &input).unwrap();
    for offset in [
        Vec3::NEG_X,
        Vec3::X,
        Vec3::NEG_Y,
        Vec3::Y,
        Vec3::NEG_Z,
        Vec3::Z,
    ] {
        let point = input.clip * (center + offset * LIGHT_RADIUS).extend(1.0);
        let uv = point.truncate().truncate() / point.w * Vec2::new(0.5, -0.5) + Vec2::splat(0.5);
        if uv.cmpge(Vec2::ZERO).all() && uv.cmplt(Vec2::ONE).all() {
            let tile = (uv * Vec2::splat(input.size[0] as f32 / TILE_SIDE as f32)).as_uvec2();
            assert!(tile.x >= low[0] && tile.x <= high[0] && tile.y >= low[1] && tile.y <= high[1]);
        }
    }
}

#[test]
fn changing_a_nonemitting_wall_invalidates_point_shadow_geometry() {
    let mut sources = LocalLightSources::default();
    sources.record_changes(true, false);
    assert_eq!(
        sources.revision, 1,
        "nonemitting caster changes invalidate shadow geometry"
    );
    assert_eq!(
        sources.lights_revision, 0,
        "unchanged emitter selection is reusable"
    );
    sources.record_changes(false, false);
    assert_eq!(
        sources.revision, 1,
        "unchanged geometry does not invalidate cached shadows"
    );
    sources.record_changes(true, true);
    assert_eq!(sources.revision, 2);
    assert_eq!(sources.lights_revision, 1);
}

#[test]
fn point_shadow_cache_tracks_nearby_interpolated_pose_without_clock_churn() {
    use std::sync::Arc;
    let mut frame = crate::ActorRenderFrame::default();
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    frame.rig.instances = Arc::from([crate::ActorGpuInstance {
        world_from_actor: identity,
        ..default()
    }]);
    frame.rig.previous_bones = Arc::from([identity]);
    frame.rig.current_bones = Arc::from([identity]);
    frame.rig.manifest = Arc::from([crate::ActorDrawManifestEntry {
        identity: crate::ActorRenderIdentity {
            session_id: 1,
            dimension: 0,
            runtime_id: 1,
            spawn_revision: 1,
            ingress_sequence: 1,
            source_tick: None,
            movement_revision: 0,
            pose_generation: 1,
            layer: 0,
        },
        rig: render_model::DIAGNOSTIC_RIG_ID,
        completed_tick: 0,
        reset_generation: 0,
        route: crate::ActorRigRoute::Diagnostic,
        instance_index: 0,
        previous_bone_base: 0,
        current_bone_base: 0,
        bone_count: 1,
    }]);
    let shadows: Vec<_> = point_shadow_matrices(Vec3::ZERO)
        .into_iter()
        .enumerate()
        .map(|(face, clip)| PointShadowView {
            clip,
            position: Vec3::ZERO,
            layer: face as u32,
        })
        .collect();
    let unchanged = near_actor_signature(&frame, &shadows);
    frame.rig.frame_generation += 1;
    Arc::make_mut(&mut frame.rig.instances)[0].partial_tick = 0.5;
    assert_eq!(near_actor_signature(&frame, &shadows), unchanged);
    Arc::make_mut(&mut frame.rig.instances)[0].world_from_actor[0][3] = 0.001;
    assert_ne!(
        near_actor_signature(&frame, &shadows),
        unchanged,
        "near-emitter motion updates caster coverage even below one world texel"
    );
    Arc::make_mut(&mut frame.rig.current_bones)[0][0][3] = 0.25;
    assert_ne!(
        near_actor_signature(&frame, &shadows),
        unchanged,
        "actual interpolation changes caster geometry"
    );
    Arc::make_mut(&mut frame.rig.instances)[0].world_from_actor[0][3] = 100.0;
    let distant = near_actor_signature(&frame, &shadows);
    Arc::make_mut(&mut frame.rig.current_bones)[0][0][3] = 0.8;
    assert_eq!(
        near_actor_signature(&frame, &shadows),
        distant,
        "unrelated distant actor movement does not invalidate point maps"
    );
}

#[test]
fn native_attenuation_is_finite_inverse_square_and_has_no_boundary_step() {
    let source = r#"
#import cinnabar::enhanced_local_lights::{point_attenuation,point_shadow_face}
@group(0) @binding(31) var<storage,read_write> result:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    let radius=__RADIUS__;
    result[0]=vec4(point_attenuation(0.0,radius),point_attenuation(1.0,radius),point_attenuation(2.0,radius),point_attenuation(radius*0.999,radius));
    result[1]=vec4(point_attenuation(radius,radius),point_attenuation(radius+4.0,radius),f32(point_shadow_face(vec3(1.0,0.0,0.0))),f32(point_shadow_face(vec3(-1.0,0.0,0.0))));
    result[2]=vec4(f32(point_shadow_face(vec3(0.0,1.0,0.0))),f32(point_shadow_face(vec3(0.0,-1.0,0.0))),f32(point_shadow_face(vec3(0.0,0.0,1.0))),f32(point_shadow_face(vec3(0.0,0.0,-1.0))));
}
"#;
    let source = source.replace("__RADIUS__", &format!("{LIGHT_RADIUS:.1}"));
    let Some(values) = super::super::post_regressions::execute(&source, 3) else {
        return;
    };
    assert!(
        values[0]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
    );
    assert!(values[0][0] <= 4.0 && values[0][0] > values[0][1]);
    assert!((values[0][1] / values[0][2] - 4.0).abs() < 0.03);
    assert!(values[0][3] < 0.000001);
    assert_eq!(&values[1][..2], &[0.0; 2]);
    assert_eq!(&values[1][2..], &[0.0, 1.0]);
    assert_eq!(values[2], [2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn point_shadow_faces_cover_each_axis_with_positive_depth() {
    let position = Vec3::new(-18.0, 64.5, 9.0);
    for (matrix, direction) in point_shadow_matrices(position).into_iter().zip([
        Vec3::X,
        -Vec3::X,
        Vec3::Y,
        -Vec3::Y,
        Vec3::Z,
        -Vec3::Z,
    ]) {
        let clip = matrix * (position + direction * 2.0).extend(1.0);
        assert!(clip.w > 0.0 && clip.z / clip.w > 0.0 && clip.z / clip.w < 1.0);
        assert!(clip.x.abs() < 0.001 && clip.y.abs() < 0.001);
    }
}

#[test]
fn indirect_light_signature_tracks_radiance_without_shadow_or_selection_order_churn() {
    let mut data = LocalLightBlock::zeroed();
    data.info[0] = 2;
    data.lights[0].position_radius = [1.0, 2.0, 3.0, 12.0];
    data.lights[0].radiance_shadow = [2.0, 1.0, 0.5, 1.0];
    data.lights[1].position_radius = [2.0, 2.0, 3.0, 12.0];
    data.lights[1].radiance_shadow = [1.0, 0.5, 0.2, 7.0];
    let signature = data.illumination_signature();
    data.lights.swap(0, 1);
    data.info[1] = 124;
    data.lights[0].radiance_shadow[3] = 0.0;
    data.lights[0].clip = [[[5.0; 4]; 4]; POINT_SHADOW_FACES];
    assert_eq!(data.illumination_signature(), signature);
    data.lights[0].radiance_shadow[0] += 0.1;
    assert_ne!(data.illumination_signature(), signature);
}
