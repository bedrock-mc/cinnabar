use super::*;

fn cube_carrier() -> assets::CompiledAssets {
    use assets::*;
    CompiledAssets {
        visuals: vec![
            BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary),
            BlockVisual {
                faces: [1, 2, 3, 4, 5, 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
        ]
        .into(),
        light_properties: vec![LightProperties::default(); 2].into(),
        hashed: Box::new([]),
        materials: (0_u32..7)
            .map(|id| Material {
                texture: TextureRef::new(0, id.saturating_sub(1)).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
            })
            .collect::<Vec<_>>()
            .into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 6,
            mips: [16, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: (0..6)
                        .flat_map(|face| {
                            (0..size * size).flat_map(move |_| [face as u8 + 1, 20, 40, 255])
                        })
                        .collect::<Vec<_>>()
                        .into(),
                })
                .collect::<Vec<_>>()
                .into(),
        })]
        .into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    }
}

#[test]
fn opaque_cube_gameplay_flags_do_not_change_held_geometry() {
    for flag in [
        assets::BlockFlags::FIRE_FLAMMABLE,
        assets::BlockFlags::FIRE_TOP_SUPPORT,
        assets::BlockFlags::SEASONAL_REPLACEABLE,
    ] {
        let mut source = cube_carrier();
        source.visuals[1].flags |= flag;
        let runtime =
            assets::RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap();
        let (geometry, _) = ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1))
            .expect("gameplay flags preserve opaque cube geometry");
        assert_eq!(geometry.vertices.len(), 36);
    }
}

#[test]
fn opaque_cube_transports_all_six_face_layers_without_sprite_extrusion() {
    let source = cube_carrier();
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap();
    let (geometry, pixels) =
        ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).unwrap();
    assert_eq!(geometry.vertices.len(), 36);
    let side = VIEWMODEL_TEXTURE_SIDE as usize;
    assert_eq!(pixels.rgba8.len(), side * side * 4);
    for face in 0..6 {
        let vertices = &geometry.vertices[face * 6..face * 6 + 6];
        let low: [usize; 2] = std::array::from_fn(|axis| {
            (vertices.iter().map(|v| v.uv[axis]).fold(f32::MAX, f32::min) * side as f32) as usize
        });
        let offset = (low[1] * side + low[0]) * 4;
        assert_eq!(
            &pixels.rgba8[offset..offset + 4],
            &[face as u8 + 1, 20, 40, 255]
        );
        let gutter = ((low[1] - 1) * side + low[0] - 1) * 4;
        assert_eq!(
            &pixels.rgba8[gutter..gutter + 4],
            &pixels.rgba8[offset..offset + 4]
        );
        for vertex in vertices {
            assert!(vertex.position.iter().all(|p| p.is_finite()));
            assert!(vertex.position[2] < 0.);
            assert!(vertex.uv.iter().all(|uv| (0.0..=1.0).contains(uv)));
        }
    }
    assert!(ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(0)).is_none());
    assert!(ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(2)).is_none());
    let mut changed = source.clone();
    changed.texture_pages[0].texture.mips[0].rgba8[0] += 1;
    let changed = assets::RuntimeAssets::decode(&assets::encode_blob(&changed).unwrap()).unwrap();
    let (changed_geometry, changed_pixels) =
        ViewmodelGeometry::opaque_cube(&changed, assets::BlockVisualId(1)).unwrap();
    assert_ne!(geometry.identity, changed_geometry.identity);
    assert_ne!(pixels.identity, changed_pixels.identity);
}

#[test]
fn opaque_cube_refuses_tint_cutout_and_fallback_visuals() {
    for flags in [
        assets::MATERIAL_FLAG_GRASS_TINT,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MATERIAL_FLAG_ROTATE_UV,
    ] {
        let mut source = cube_carrier();
        source.materials[1].flags = flags;
        let runtime =
            assets::RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap();
        assert!(ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).is_none());
    }
    let mut source = cube_carrier();
    source.visuals[1].support = assets::VisualSupport::VanillaFallback;
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap();
    assert!(ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).is_none());
    let mut source = cube_carrier();
    source.texture_pages[0].texture.mips[0].rgba8[3] = 0;
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap();
    assert!(ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).is_none());
}

#[test]
fn neutral_anchor_pins_all_independent_camera_local_corners() {
    let expected = [
        [0.40368183, -0.811_525_9, -0.503_502_2],
        [0.60918505, -0.92329095, -0.48905805],
        [0.668_541_3, -0.38853702, -0.99879473],
        [0.874_044_5, -0.50030211, -0.98435057],
        [0.333_642_1, -0.96172657, -0.66923036],
        [0.539_145_3, -1.073_491_7, -0.654_786_2],
        [0.59850155, -0.538_737_7, -1.164_522_9],
        [0.804_004_8, -0.650_502_8, -1.150_078_8],
    ];
    let transform = geometry::neutral_arm_transform();
    let mut index = 0;
    for x in [-3.0, 1.0] {
        for y in [-2.0, 10.0] {
            for z in [-2.0, 2.0] {
                let actual = transform.transform_point3(bevy::math::Vec3::new(x, y, z));
                for axis in 0..3 {
                    assert!((actual[axis] - expected[index][axis]).abs() < 0.000002);
                }
                index += 1;
            }
        }
    }
}

#[test]
fn completion_is_exact_and_invalidation_rejects_old_callbacks() {
    let gate = ViewmodelCompletionGate::default();
    let token = test_token();
    gate.select(Some(token));
    assert!(!gate.completed(token));
    let reservation = gate.reserve(token).unwrap();
    gate.select(None);
    gate.select(Some(token));
    assert!(!gate.complete(reservation));
    assert!(!gate.completed(token));
    let reservation = gate.reserve(token).unwrap();
    assert!(gate.complete(reservation));
    assert!(gate.completed(token));
    let mut resized = token;
    resized.viewport[0] += 1;
    gate.select(Some(resized));
    assert!(!gate.completed(resized));
}

#[test]
fn completed_gpu_hand_is_revoked_on_missing_coverage_or_epoch_exhaustion() {
    let gate = ViewmodelCompletionGate::default();
    let token = test_token();
    gate.select(Some(token));
    let reserved = gate.reserve(token).unwrap();
    assert!(gate.complete(reserved));
    gate.reject(token);
    assert!(!gate.completed(token));
    assert_eq!(gate.rejection_count(), 1);
    assert!(gate.rejected(token));
    let reserved = gate.reserve(token).unwrap();
    assert!(gate.complete(reserved));
    assert!(!gate.rejected(token));
    gate.0.lock().unwrap().epoch = u64::MAX;
    gate.select(None);
    gate.select(Some(token));
    assert!(gate.reserve(token).is_none());
    assert!(!gate.completed(token));
}

#[test]
fn token_lifetime_view_and_material_changes_each_require_fresh_completion() {
    let base = test_token();
    for changed in [
        ViewmodelToken { session: 2, ..base },
        ViewmodelToken {
            actor_session: 3,
            ..base
        },
        ViewmodelToken {
            dimension: -1,
            ..base
        },
        ViewmodelToken { spawn: 4, ..base },
        ViewmodelToken { samples: 4, ..base },
        ViewmodelToken { hdr: true, ..base },
        ViewmodelToken {
            skin: [7; 32],
            ..base
        },
        ViewmodelToken {
            geometry: [8; 32],
            ..base
        },
        ViewmodelToken {
            revision: 2,
            ..base
        },
    ] {
        let gate = ViewmodelCompletionGate::default();
        gate.select(Some(base));
        let reserved = gate.reserve(base).unwrap();
        gate.select(Some(changed));
        assert!(!gate.complete(reserved));
        assert!(!gate.completed(changed));
        assert!(gate.complete(gate.reserve(changed).unwrap()));
    }
}

#[test]
fn depth_limit_counts_samples_and_overflow() {
    assert_eq!(viewmodel_depth_bytes([1920, 1080], 4), Some(33_177_600));
    assert_eq!(viewmodel_depth_bytes([0, 1080], 1), None);
    assert_eq!(viewmodel_depth_bytes([u32::MAX, u32::MAX], 4), None);
    assert_eq!(viewmodel_depth_bytes([8192, 8192], 1), None);
}

#[test]
fn skin_fractional_alpha_is_not_silently_quantized() {
    let pixels: Arc<[u8]> = vec![255; VIEWMODEL_TEXTURE_BYTES].into();
    assert!(ViewmodelSkin::new(pixels.clone(), [1; 32]).is_some());
    let mut fractional = pixels.to_vec();
    fractional[3] = 128;
    assert!(ViewmodelSkin::new(fractional.into(), [2; 32]).is_none());
    assert!(ViewmodelSkin::new(Arc::from([255; 4]), [3; 32]).is_none());
}

fn test_token() -> ViewmodelToken {
    ViewmodelToken {
        session: 1,
        actor_session: 9,
        dimension: 0,
        runtime: 2,
        spawn: 3,
        owner: bevy::prelude::Entity::from_raw_u32(0).unwrap(),
        viewport: [1920, 1080],
        samples: 1,
        hdr: false,
        skin: [4; 32],
        geometry: [5; 32],
        revision: 1,
    }
}

fn profile() -> assets::EntityGeometry {
    use assets::{
        EntityGeometry, EntityGeometryBone, EntityGeometryCube, EntityGeometryScalar as S,
        EntityGeometryUv,
    };
    let vec = |value: [f32; 3]| value.map(|v| S::new(v).unwrap());
    let bones = [
        ("root", None, [0., 0., 0.]),
        ("waist", Some("root"), [0., 12., 0.]),
        ("body", Some("waist"), [0., 24., 0.]),
        ("rightArm", Some("body"), [-5., 22., 0.]),
        ("rightSleeve", Some("rightArm"), [-5., 22., 0.]),
    ]
    .into_iter()
    .map(|(name, parent, pivot)| {
        let cubes = if matches!(name, "rightArm" | "rightSleeve") {
            vec![EntityGeometryCube {
                origin: vec([-8., 12., -2.]),
                size: vec([4., 12., 4.]),
                pivot: vec([0.; 3]),
                rotation: vec([0.; 3]),
                uv: EntityGeometryUv::Box(
                    [40., if name == "rightArm" { 16. } else { 32. }].map(|v| S::new(v).unwrap()),
                ),
                inflate: S::new(if name == "rightArm" { 0. } else { 0.25 }).unwrap(),
                mirror: false,
            }]
        } else {
            Vec::new()
        };
        EntityGeometryBone {
            name: name.into(),
            binding: None,
            texture_meshes: Box::new([]),
            parent: parent.map(Into::into),
            pivot: Some(vec(pivot)),
            rotation: None,
            bind_pose_rotation: None,
            inflate: None,
            mirror: None,
            never_render: None,
            reset: None,
            cubes: cubes.into(),
        }
    })
    .collect::<Vec<_>>()
    .into();
    EntityGeometry {
        visible_bounds: None,
        identifier: "geometry.humanoid.custom".into(),
        inherits: None,
        source_index: 0,
        texture_width: VIEWMODEL_TEXTURE_SIDE as u16,
        texture_height: VIEWMODEL_TEXTURE_SIDE as u16,
        bones,
    }
}

#[test]
fn arm_and_sleeve_share_one_parent_transform_and_distinct_uv_faces() {
    let model = profile();
    let mesh = geometry::validated_geometry(&model, [5; 32]).unwrap();
    assert_eq!(mesh.vertices.len(), 72);
    let transform = geometry::neutral_arm_transform();
    assert_eq!(
        mesh.vertices[0].position,
        transform
            .transform_point3(bevy::math::Vec3::new(-3., 10., -2.))
            .to_array()
    );
    assert_eq!(
        mesh.vertices[0].uv,
        [
            44. / VIEWMODEL_TEXTURE_SIDE as f32,
            20. / VIEWMODEL_TEXTURE_SIDE as f32
        ]
    );
    assert_eq!(
        mesh.vertices[1].uv,
        [
            48. / VIEWMODEL_TEXTURE_SIDE as f32,
            20. / VIEWMODEL_TEXTURE_SIDE as f32
        ]
    );
    assert_eq!(
        mesh.vertices[2].uv,
        [
            48. / VIEWMODEL_TEXTURE_SIDE as f32,
            32. / VIEWMODEL_TEXTURE_SIDE as f32
        ]
    );
    assert_eq!(
        mesh.vertices[36].uv,
        [
            44. / VIEWMODEL_TEXTURE_SIDE as f32,
            36. / VIEWMODEL_TEXTURE_SIDE as f32
        ]
    );
    assert_eq!(
        mesh.vertices[36].position,
        transform
            .transform_point3(bevy::math::Vec3::new(-3.25, 10.25, -2.25))
            .to_array()
    );
    for face in mesh.vertices.chunks_exact(6) {
        assert_eq!(face[0].position, face[3].position);
        assert_eq!(face[2].position, face[4].position);
        assert_eq!(face[0].uv, face[3].uv);
        assert_eq!(face[2].uv, face[4].uv);
        let a = bevy::math::Vec3::from(face[1].position) - bevy::math::Vec3::from(face[0].position);
        let b = bevy::math::Vec3::from(face[2].position) - bevy::math::Vec3::from(face[0].position);
        assert!(a.cross(b).length_squared() > 0.);
        assert!(
            face.iter()
                .all(|vertex| vertex.position.iter().all(|v| v.is_finite())
                    && vertex.uv.iter().all(|v| (0.0..=1.0).contains(v)))
        );
    }
}

#[test]
fn unverified_profiles_reject_instead_of_recalibrating_the_anchor() {
    use assets::EntityGeometryScalar as S;
    let mut model = profile();
    model.bones[3].cubes[0].size[0] = S::new(3.).unwrap();
    assert!(geometry::validated_geometry(&model, [5; 32]).is_none());
    let mut model = profile();
    model.bones[2].rotation = Some([0., 0., 1.].map(|v| S::new(v).unwrap()));
    assert!(geometry::validated_geometry(&model, [5; 32]).is_none());
    let mut model = profile();
    model.bones[4].parent = Some("body".into());
    assert!(geometry::validated_geometry(&model, [5; 32]).is_none());
    let mut model = profile();
    model.texture_width = 128;
    assert!(geometry::validated_geometry(&model, [5; 32]).is_none());
    let mut model = profile();
    model.bones[3].never_render = Some(true);
    assert!(geometry::validated_geometry(&model, [5; 32]).is_none());
}

#[test]
fn reverse_z_projection_is_private_aspect_correct_and_world_fov_independent() {
    let projection = hand_projection([1920, 1080]);
    let nearby = projection.project_point3(bevy::math::Vec3::new(0., 0., -0.05));
    assert!(
        (0.0..=1.0).contains(&nearby.z),
        "nearby hand geometry is clipped with reverse-Z depth {}",
        nearby.z
    );
    let near = projection.project_point3(bevy::math::Vec3::new(
        0.,
        0.,
        -render_api::CAMERA_NEAR_PLANE_BLOCKS,
    ));
    assert!((near.z - 1.).abs() < 0.000001);
    let far = projection.project_point3(bevy::math::Vec3::new(0., 0., -1000.));
    assert!(far.z > 0. && far.z < 0.001);
    assert!((projection.y_axis.y / projection.x_axis.x - 1920. / 1080.).abs() < 0.000001);
}

fn fallback_input() -> render_model::UiRenderInput {
    use render_model::*;
    UiRenderInput {
        revision: 1,
        viewport_size: test_token().viewport,
        safe_area: [0; 4],
        vertices: [
            ([1., 2.], [4., 8.]),
            ([3., 2.], [12., 8.]),
            ([3., 4.], [12., 24.]),
            ([1., 4.], [4., 24.]),
        ]
        .map(|(position, uv)| UiRenderVertex {
            position,
            clip_z: 0.0,
            clip_w: 1.0,
            uv,
            color: [255; 4],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        })
        .into(),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 1920, 1080),
            0,
            6,
            UI_BLEND_ALPHA,
        )]),
        textures: Arc::new(
            render_model::UiTextureCatalog::new(
                vec![
                    render_model::UiTexturePage::owned(
                        [VIEWMODEL_TEXTURE_SIDE; 2],
                        vec![255; VIEWMODEL_TEXTURE_BYTES].into(),
                    )
                    .unwrap(),
                ],
                1,
            )
            .unwrap(),
        ),
    }
}
fn fallback_scene(gate: &ViewmodelCompletionGate) -> ViewmodelScene {
    let mut scene = ViewmodelScene::default();
    let skin = ViewmodelSkin::new(vec![255; VIEWMODEL_TEXTURE_BYTES].into(), [4; 32]).unwrap();
    let geometry = geometry::validated_geometry(&profile(), [5; 32]).unwrap();
    assert!(scene.publish(test_token(), &skin, &geometry, gate));
    scene
}
#[test]
fn cube_fallback_binds_rotated_edge_quad_but_never_relaxes_empty_hand() {
    let runtime =
        assets::RuntimeAssets::decode(&assets::encode_blob(&cube_carrier()).unwrap()).unwrap();
    let (geometry, pixels) =
        ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).unwrap();
    let token = ViewmodelToken {
        geometry: geometry.identity,
        skin: pixels.identity,
        ..test_token()
    };
    let gate = ViewmodelCompletionGate::default();
    let mut input = fallback_input();
    let rotation = Mat2::from_angle(-0.04);
    let mut vertices = input.vertices.to_vec();
    for vertex in &mut vertices {
        vertex.position = (rotation * (Vec2::from(vertex.position) - Vec2::new(2., 3.))
            + Vec2::new(1920., 1079.))
        .to_array();
    }
    input.vertices = vertices.into();
    let mut scene = ViewmodelScene::default();
    assert!(scene.publish(token, &pixels, &geometry, &gate));
    assert!(scene.is_opaque_cube());
    assert!(scene.bind_cube_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
    assert_eq!(scene.frame.as_ref().unwrap().fallback, Some((1, 0, 0)));
    assert!(!gate.completed(token));
    assert!(gate.complete(gate.reserve(token).unwrap()));
    assert!(gate.completed(token));
    assert!(
        input
            .vertices
            .iter()
            .any(|vertex| vertex.position[0] > 1920.)
    );
    let mut empty = fallback_scene(&gate);
    assert!(!empty.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
    assert!(empty.frame.is_none());
    assert!(!gate.completed(token));
    for hostile in 0..7 {
        assert!(scene.publish(token, &pixels, &geometry, &gate));
        let mut invalid = input.clone();
        match hostile {
            0 => {
                let mut vertices = invalid.vertices.to_vec();
                vertices[2].position[0] += 1.;
                invalid.vertices = vertices.into();
            }
            1 => invalid.indices = Arc::from([0, 1, 2, 0, 2, 1]),
            2 => invalid.batches = Arc::from([invalid.batches[0], invalid.batches[0]]),
            3 => {
                let mut vertices = invalid.vertices.to_vec();
                vertices[0].style_flags = 1;
                invalid.vertices = vertices.into();
            }
            4 => {
                let mut vertices = invalid.vertices.to_vec();
                vertices[0].uv[0] += 1.0;
                invalid.vertices = vertices.into();
            }
            5 => {
                let mut batch = invalid.batches[0];
                batch.scissor.width = 0;
                invalid.batches = Arc::from([batch]);
            }
            _ => {
                let mut vertices = invalid.vertices.to_vec();
                for vertex in &mut vertices {
                    vertex.position[0] += 100.;
                }
                invalid.vertices = vertices.into();
            }
        }
        assert!(!scene.bind_cube_cpu_fallback(&invalid, 0, [4, 8, 12, 24], &gate));
        assert!(scene.frame.is_none());
        assert!(!gate.completed(token));
    }
    let mut empty = fallback_scene(&gate);
    assert!(!empty.bind_cube_cpu_fallback(&fallback_input(), 0, [4, 8, 12, 24], &gate));
    assert!(empty.frame.is_none());
}
#[test]
fn cpu_fallback_requires_exact_float_texel_edges_not_rounded_model_centers() {
    let gate = ViewmodelCompletionGate::default();
    let mut scene = fallback_scene(&gate);
    let mut input = fallback_input();
    assert!(scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));

    let mut vertices = input.vertices.to_vec();
    vertices[0].uv[0] += 0.5;
    input.vertices = vertices.into();
    assert!(!scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
    assert!(scene.frame.is_none());
}

#[test]
fn cpu_fallback_join_is_unique_bounded_and_ui_revision_does_not_reset_lifetime_completion() {
    let gate = ViewmodelCompletionGate::default();
    let mut scene = fallback_scene(&gate);
    let mut input = fallback_input();
    assert!(scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
    assert_eq!(scene.frame.as_ref().unwrap().fallback, Some((1, 0, 0)));
    assert!(gate.complete(gate.reserve(test_token()).unwrap()));
    input.revision = 2;
    assert!(scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
    assert!(gate.completed(test_token()));
    assert_eq!(scene.frame.as_ref().unwrap().fallback, Some((2, 0, 0)));
    for hostile in [0, 1, 2, 3] {
        let mut scene = fallback_scene(&gate);
        let mut input = fallback_input();
        match hostile {
            0 => {
                input.indices = Arc::from([0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3]);
                let mut batch = input.batches[0];
                batch.index_count = 12;
                input.batches = Arc::from([batch]);
            }
            1 => input.indices = Arc::from([0, 1, 2, 0, 2, 99]),
            2 => input.batches = Arc::from([input.batches[0], input.batches[0]]),
            _ => input.viewport_size[0] += 1,
        }
        assert!(!scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
        assert!(scene.frame.is_none());
        assert!(!gate.completed(test_token()));
    }
}

#[test]
fn actual_renderer_startup_revokes_pending_callback_and_equal_token_publish_recovers() {
    use bevy::{
        app::SubApp,
        ecs::schedule::Schedule,
        render::{ExtractSchedule, Render, RenderApp, RenderStartup},
    };
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut render_app = SubApp::new();
    render_app
        .insert_resource(bevy::render::renderer::RenderDevice::from(device))
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule));
    let mut app = App::new();
    app.insert_resource(Assets::<Shader>::default())
        .insert_sub_app(RenderApp, render_app);
    app.add_plugins(crate::viewmodel_render::ViewmodelRenderPlugin);
    app.finish();
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    let mut scene = fallback_scene(&gate);
    let token = test_token();
    let old = gate.reserve(token).unwrap();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .run_schedule(RenderStartup);
    assert!(!gate.complete(old));
    assert!(gate.reserve(token).is_none());
    assert!(!gate.completed(token));
    let frame = scene.frame.as_ref().unwrap().clone();
    assert!(scene.publish(token, &frame.skin, &frame.geometry, &gate));
    assert!(!gate.complete(old));
    let fresh = gate.reserve(token).unwrap();
    assert!(!gate.complete(old));
    assert!(gate.complete(fresh));
    assert!(gate.completed(token));
}

#[test]
fn fallback_identity_is_logical_even_when_its_layer_is_in_another_bucket() {
    let gate = ViewmodelCompletionGate::default();
    let mut scene = fallback_scene(&gate);
    let mut input = fallback_input();
    input.textures = Arc::new(
        render_model::UiTextureCatalog::new(
            vec![
                render_model::UiTexturePage::owned([1024, 1024], vec![255; 1024 * 1024 * 4].into())
                    .unwrap(),
                render_model::UiTexturePage::owned(
                    [VIEWMODEL_TEXTURE_SIDE; 2],
                    vec![255; VIEWMODEL_TEXTURE_BYTES].into(),
                )
                .unwrap(),
            ],
            2,
        )
        .unwrap(),
    );
    let mut batch = input.batches[0];
    batch.texture_page = 1;
    input.batches = Arc::from([batch]);
    assert_eq!(
        input.textures.plan().locations()[1],
        render_model::UiTextureLocation {
            bucket: 1,
            layer: 0
        }
    );
    assert!(scene.bind_cpu_fallback(&input, 1, [4, 8, 12, 24], &gate));
    assert_eq!(scene.frame.as_ref().unwrap().fallback, Some((1, 0, 1)));
    assert!(!scene.bind_cpu_fallback(&input, 0, [4, 8, 12, 24], &gate));
}

// The held-block viewmodel reads the base carrier, and even against session
// assets a server overlay cube (page-1 material, vanilla-fallback support) is
// refused, so overlay ids never build viewmodel geometry.
#[test]
fn opaque_cube_refuses_server_block_overlay_ids() {
    use assets::*;
    let base = RuntimeAssets::decode(&encode_blob(&cube_carrier()).unwrap()).unwrap();
    let base_count = base.visual_count() as u32;
    // An id past the base carrier is out of range on the base assets.
    assert!(ViewmodelGeometry::opaque_cube(&base, BlockVisualId(base_count)).is_none());

    let mip = |size: u32| TextureMip {
        size,
        rgba8: (0..size * size)
            .flat_map(|_| [200, 200, 200, 255])
            .collect(),
    };
    let overlay = BlockOverlay {
        visuals: vec![BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: VisualSupport::VanillaFallback,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }],
        light_properties: vec![LightProperties::OPAQUE_DARK],
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }],
        texture: Some(TextureArray {
            layers: 1,
            mips: [16, 8, 4, 2, 1].into_iter().map(mip).collect(),
        }),
        ..BlockOverlay::default()
    };
    let session = base.with_block_overlay(base_count, &overlay).unwrap();
    // The overlay id resolves on the session carrier, but its vanilla-fallback
    // support keeps it out of the opaque-cube viewmodel.
    assert!(
        session
            .resolve(NetworkIdMode::Sequential, base_count)
            .is_known()
    );
    assert!(ViewmodelGeometry::opaque_cube(&session, BlockVisualId(base_count)).is_none());
}

#[test]
fn review_render_cube_uvs_span_complete_source_texels() {
    let runtime =
        assets::RuntimeAssets::decode(&assets::encode_blob(&cube_carrier()).unwrap()).unwrap();
    let (geometry, skin) =
        ViewmodelGeometry::opaque_cube(&runtime, assets::BlockVisualId(1)).unwrap();
    for face in geometry.vertices.chunks_exact(6) {
        for axis in 0..2 {
            let low = face
                .iter()
                .map(|vertex| vertex.uv[axis])
                .fold(f32::MAX, f32::min);
            let high = face
                .iter()
                .map(|vertex| vertex.uv[axis])
                .fold(f32::MIN, f32::max);
            assert_eq!(
                (high - low) * (skin.rgba8.len() / 4).isqrt() as f32,
                runtime.texture_pages()[0].texture.mips[0].size as f32
            );
        }
    }
}

#[test]
fn review_render_fixed_arm_rejects_bindings_and_texture_meshes() {
    let mut geometry = profile();
    geometry.bones[3].binding = Some("q.item_slot_to_bone_name(context.item_slot)".into());
    assert!(geometry::validated_geometry(&geometry, [5; 32]).is_none());
    let mut geometry = profile();
    let zero = assets::EntityGeometryScalar::new(0.0).unwrap();
    geometry.bones[3].texture_meshes = Box::new([assets::EntityGeometryTextureMesh {
        local_pivot: [zero; 3],
        position: [zero; 3],
        rotation: [zero; 3],
        scale: assets::EntityGeometryTextureMesh::DEFAULT_SCALE,
        use_pixel_depth: true,
        texture: "default".into(),
    }]);
    assert!(geometry::validated_geometry(&geometry, [5; 32]).is_none());
}
