//! Per-view Enhanced uniforms, targets and cached local-light preparation.

use super::*;

/// Collects the camera matrices and render target dimensions.
fn view_inputs(
    view: &ExtractedView,
    seconds: f32,
    jitter: Option<&bevy::render::camera::TemporalJitter>,
) -> ViewInputs {
    let world_from_view = view.world_from_view.to_matrix();
    let mut clip_from_world = view
        .clip_from_world
        .unwrap_or(view.clip_from_view * world_from_view.inverse());
    let projection = view.clip_from_view;
    if let Some(jitter) = jitter {
        let mut projection = projection;
        jitter.jitter_projection(&mut projection, view.viewport.zw().as_vec2());
        clip_from_world = projection * world_from_view.inverse();
    }
    ViewInputs {
        clip_from_world,
        world_from_clip: clip_from_world.inverse(),
        camera: view.world_from_view.translation(),
        near: projection.w_axis.z,
        viewport: [view.viewport.z, view.viewport.w],
        seconds,
    }
}

fn cloud_shadow_sampling_enabled(settings: &EnhancedRendering, effects_ready: bool) -> bool {
    settings.volumetric_clouds && effects_ready
}

fn wind_shadow_update_due(seconds: f32, previous: f32) -> bool {
    let interval = crate::enhanced::local_lights::POINT_SHADOW_ANIMATION_INTERVAL;
    seconds.is_finite()
        && (!previous.is_finite()
            || seconds < previous
            || (seconds / interval).floor() > (previous / interval).floor())
}

/// Prepares uniforms and targets only for opted-in cameras.
#[derive(SystemParam)]
pub(crate) struct LocalLightingInputs<'w> {
    sources: Res<'w, crate::enhanced::local_lights::LocalLightSources>,
    geometry: Res<'w, crate::enhanced::indirect::IndirectGeometry>,
    coverage: Res<'w, crate::chunk::ChunkResidentCoverage>,
    assets: Res<'w, crate::ChunkTextureAssets>,
    tints: Res<'w, crate::ChunkBiomeTints>,
    noise: Res<'w, crate::enhanced::cloud_noise::CloudNoiseVolume>,
    scattering: Res<'w, crate::enhanced::multiple_scattering::MultipleScattering>,
    actors: Option<Res<'w, crate::ActorRenderFrame>>,
    environment: Option<Res<'w, crate::AtmosphereViewInputs>>,
    pipelines: Res<'w, crate::enhanced::shadows::EnhancedShadowPipelines>,
}

fn render_dimension(environment: Option<&crate::AtmosphereViewInputs>) -> Option<i32> {
    environment.map(|environment| environment.dimension)
}

struct CaptureLighting {
    parent: Entity,
    frame: Buffer,
    shadow: TextureView,
    point_shadow: TextureView,
    cloud: TextureView,
    lights: Buffer,
    indirect: Buffer,
    submitted: std::sync::Arc<std::sync::atomic::AtomicU64>,
    submission: u64,
    needs_shadow: bool,
    ready: bool,
}

/// Prepares uniforms and targets only for opted-in cameras.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_enhanced_views(
    mut state: ResMut<EnhancedViews>,
    gpu: Res<EnhancedGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    depth_pipelines: Res<crate::enhanced::depth::EnhancedDepthPipelines>,
    post_pipelines: Res<crate::enhanced::post::EnhancedPostPipelines>,
    local_shadow_pipeline: Res<crate::enhanced::local_shadow_history::LocalShadowPipeline>,
    atmosphere: Option<Res<AtmosphereFrame>>,
    time: Res<Time>,
    probe: Res<crate::enhanced::probes::ProbeGpu>,
    probe_origin: Res<crate::enhanced::probes::ProbeOrigin>,
    probe_faces: Query<(), With<crate::enhanced::probes::ProbeFace>>,
    local: LocalLightingInputs,
    views: Query<(
        Entity,
        &ExtractedView,
        &ViewTarget,
        &EnhancedRendering,
        Option<&bevy::render::camera::TemporalJitter>,
        Option<&bevy::render::camera::ExtractedCamera>,
    )>,
) {
    state
        .0
        .retain(|entity, _| views.contains(*entity) || probe_faces.contains(*entity));
    let atmosphere = atmosphere.map(|frame| *frame).unwrap_or_default();
    let seconds = time.elapsed_secs_wrapped();
    let view_layout = pipeline_cache.get_bind_group_layout(&enhanced_view_layout());
    let caster_layout = pipeline_cache.get_bind_group_layout(&enhanced_caster_layout());
    let dimension = render_dimension(local.environment.as_deref());
    // Parent resources must describe this render frame before a cube view binds them.
    for reflection_capture in [false, true] {
        for (entity, view, target, settings, jitter, camera) in &views {
            if settings.reflection_capture != reflection_capture {
                continue;
            }
            let capture_lighting = if reflection_capture {
                views.iter().find_map(
                    |(parent, parent_view, _, parent_settings, _, parent_camera)| {
                        if parent_settings.reflection_capture
                            || Some(parent_view.retained_view_entity.main_entity.id())
                                != probe_origin.source
                            || !camera
                                .zip(parent_camera)
                                .is_some_and(|(capture, parent)| capture.order > parent.order)
                        {
                            return None;
                        }
                        let parent_state = state.0.get(&parent)?;
                        let needs_directional = parent_state.frame_data.flags.x
                            & crate::enhanced::frame::FEATURE_SHADOWS
                            != 0;
                        let needs_point = parent_state.settings.shadows
                            && parent_state.local_lights.selected_shadow_count() > 0;
                        let needs_shadow = needs_directional || needs_point;
                        Some(CaptureLighting {
                            parent,
                            frame: parent_state.frame.clone(),
                            shadow: parent_state
                                .shadow
                                .as_ref()
                                .map_or(&gpu.fallback_shadow, |shadow| &shadow.array)
                                .clone(),
                            point_shadow: parent_state
                                .point_shadow
                                .as_ref()
                                .map_or(&gpu.fallback_shadow, |shadow| &shadow.array)
                                .clone(),
                            cloud: parent_state
                                .post
                                .as_ref()
                                .map_or(&gpu.fallback_colour, |post| &post.cloud_shadow)
                                .clone(),
                            lights: parent_state.local_lights.buffer.clone(),
                            indirect: parent_state.indirect.buffer.clone(),
                            submitted: parent_state.shadow_submitted.clone(),
                            submission: u64::from(parent_state.history.index) + 1,
                            needs_shadow,
                            ready: !needs_shadow
                                || ((!needs_directional || parent_state.shadow.is_some())
                                    && (!needs_point || parent_state.point_shadow.is_some())
                                    && local.pipelines.ready(&pipeline_cache)),
                        })
                    },
                )
            } else {
                None
            };
            let inputs = view_inputs(view, seconds, jitter);
            let mut frame_settings = *settings;
            if reflection_capture {
                frame_settings.shadows = false;
            }
            let lighting_ready = depth_pipelines.ready(&pipeline_cache)
                && post_pipelines.lighting_ready(&pipeline_cache)
                && local_shadow_pipeline.ready(&pipeline_cache)
                && (!settings.shadows || local.pipelines.ready(&pipeline_cache))
                && local.noise.ready()
                && local.scattering.ready();
            let (mut frame, fits) = build_frame(&inputs, &frame_settings, &atmosphere);
            if !lighting_ready {
                frame.atmosphere.x = 0.0;
            }
            let state = state
                .0
                .entry(entity)
                .or_insert_with(|| EnhancedViewGpu::new(&device, *settings));
            if state.settings.reflection_capture != reflection_capture {
                *state = EnhancedViewGpu::new(&device, *settings);
            }
            let settings_changed = state.settings != *settings;
            state.settings = *settings;
            state
                .shadow_submitted
                .store(0, std::sync::atomic::Ordering::Relaxed);
            state.capture_parent = capture_lighting.as_ref().map(|lighting| lighting.parent);
            state.capture_lighting_ready = !reflection_capture
                || capture_lighting
                    .as_ref()
                    .is_some_and(|lighting| lighting.ready);
            state.capture_shadow_submission = capture_lighting.as_ref().and_then(|lighting| {
                lighting
                    .needs_shadow
                    .then(|| (lighting.submitted.clone(), lighting.submission))
            });
            state.camera_clip = inputs.clip_from_world;
            if !reflection_capture
                && target.is_hdr()
                && lighting_ready
                && state.frame_data.projection.w < 0.5
            {
                state
                    .history
                    .submitted
                    .store(false, std::sync::atomic::Ordering::Release);
                if let Some(post) = &state.post {
                    post.atmosphere_cache.invalidate_radiance();
                }
            }
            let previous = state.history.previous;
            let previous_seconds = state.history.seconds;
            let valid = state
                .history
                .advance(view, inputs.clip_from_world, &frame, seconds)
                && !settings_changed;
            frame.previous_clip_from_world = previous;
            frame.temporal = Vec4::new(
                state.history.index as f32,
                f32::from(u8::from(
                    crate::enhanced::temporal::temporal_history_allowed(
                        valid,
                        settings_changed,
                        settings,
                    ),
                )),
                time.delta_secs().clamp(0.001, 0.1),
                settings.shadow_debug as u32 as f32,
            );
            frame.atmosphere.y = frame.temporal.z;
            frame.probe = probe_origin
                .position
                .extend(if settings.reflection_capture {
                    -1.0
                } else if probe.is_current(&probe_origin) {
                    32.0
                } else {
                    0.0
                });
            let cloud_center = (inputs.camera.xz() / 64.0).floor() * 64.0;
            frame.cloud_shadow = Vec4::new(
                cloud_center.x,
                cloud_center.y,
                512.0,
                f32::from(u8::from(cloud_shadow_sampling_enabled(
                    settings,
                    lighting_ready,
                ))),
            );
            if reflection_capture {
                frame.cloud_shadow.w = 0.0;
            }
            let size = [
                target.main_texture().width(),
                target.main_texture().height(),
            ];
            if !settings.reflection_capture
                && state.post.as_ref().is_none_or(|post| post.size != size)
            {
                state.post = Some(crate::enhanced::targets::PostTargets::new(
                    &device, size, settings,
                ));
                frame.temporal.y = 0.0;
            }
            if let Some(post) = &mut state.post {
                post.prepare(&device, settings);
                post.prepare_atmosphere(&frame);
            }
            state.cascades.clear();
            if !reflection_capture {
                state.cascades.extend(fits.iter().map(|fit| fit.bounds));
            }
            frame.projection.w = f32::from(u8::from(
                !settings.reflection_capture && target.is_hdr() && lighting_ready,
            ));
            queue.write_buffer(&state.frame, 0, bytemuck::bytes_of(&frame));
            state.frame_data = frame;
            if !reflection_capture {
                queue.write_buffer(
                    &state.depth_caster,
                    0,
                    bytemuck::bytes_of(&CasterUniformGpu {
                        clip_from_world: inputs.clip_from_world,
                        params: Vec4::new(seconds, frame.ambient_colour.w, 0.0, 0.0),
                        flags: UVec4::new(frame.flags.x, 0, 0, 0),
                        previous_clip_from_world: previous,
                        previous_params: Vec4::new(
                            previous_seconds,
                            inputs.near,
                            0.0,
                            f32::from(u8::from(valid)),
                        ),
                        local_light: Vec4::ZERO,
                        padding: [Vec4::ZERO; 4],
                    }),
                );
                let mut casters = [CasterUniformGpu {
                    clip_from_world: Mat4::IDENTITY,
                    params: Vec4::ZERO,
                    flags: UVec4::ZERO,
                    previous_clip_from_world: Mat4::IDENTITY,
                    previous_params: Vec4::ZERO,
                    local_light: Vec4::ZERO,
                    padding: [Vec4::ZERO; 4],
                }; crate::enhanced::MAX_SHADOW_CASCADES as usize];
                for (slot, fit) in casters.iter_mut().zip(&fits) {
                    *slot = CasterUniformGpu {
                        clip_from_world: fit.clip_from_world,
                        params: Vec4::new(seconds, frame.ambient_colour.w, 0.0, 0.0),
                        flags: UVec4::new(frame.flags.x, 0, 0, 0),
                        previous_clip_from_world: Mat4::IDENTITY,
                        previous_params: Vec4::ZERO,
                        local_light: Vec4::ZERO,
                        padding: [Vec4::ZERO; 4],
                    };
                }
                queue.write_buffer(
                    &state.casters,
                    0,
                    bytemuck::cast_slice(&casters[..fits.len()]),
                );
            }

            let resolution = frame.flags.z;
            let cascades = frame.flags.y;
            state.local_lights.set_quality(settings.quality);
            if frame.flags.x & crate::enhanced::frame::FEATURE_SHADOWS == 0 {
                state.shadow = None;
            } else if state.shadow.as_ref().is_none_or(|shadow| {
                shadow.resolution != resolution || shadow.layers.len() as u32 != cascades
            }) {
                state.shadow = Some(ShadowTargets::new(&device, resolution, cascades));
            }

            if !reflection_capture
                && state.local_lights.prepare(
                    &device,
                    &queue,
                    &local.sources,
                    dimension,
                    inputs.camera,
                    view.clip_from_world.unwrap_or_else(|| {
                        view.clip_from_view * view.world_from_view.to_matrix().inverse()
                    }),
                    inputs.viewport,
                    0,
                    false,
                    settings.shadows && local.pipelines.ready(&pipeline_cache),
                )
            {
                state.view_binding_key = None;
            }
            let point_resolution = state.local_lights.shadow_resolution();
            let point_layers = (state.local_lights.shadow_count()
                * crate::enhanced::local_lights::POINT_SHADOW_FACES)
                as u32;
            if reflection_capture
                || !settings.shadows
                || state.local_lights.selected_shadow_count() == 0
            {
                state.point_shadow = None;
            } else if state.point_shadow.as_ref().is_none_or(|shadow| {
                shadow.resolution != point_resolution || shadow.layers.len() as u32 != point_layers
            }) {
                state.point_shadow =
                    Some(ShadowTargets::new(&device, point_resolution, point_layers));
                state.local_lights.dirty = true;
                state
                    .local_lights
                    .submitted
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
            let actor_signature = local
                .actors
                .as_ref()
                .filter(|_| !reflection_capture)
                .map_or(0, |actors| {
                    crate::enhanced::local_lights::near_actor_signature(
                        actors,
                        &state.local_lights.shadows,
                    )
                });
            let actor_changed = actor_signature != state.actor_shadow_signature;
            state.local_lights.dirty |= actor_changed || settings_changed;
            state.local_lights.dirty |= state.caster_material_key != Some(gpu.materials.id());
            if settings.waving && wind_shadow_update_due(seconds, state.point_shadow_seconds) {
                state.local_lights.dirty = true;
            }
            state.actor_shadow_signature = actor_signature;
            if state.local_lights.dirty && settings.shadows && !reflection_capture {
                state.point_shadow_seconds = seconds;
                for (index, light) in state.local_lights.shadows.iter().enumerate() {
                    queue.write_buffer(
                        &state.casters,
                        (cascades as u64 + index as u64) * CASTER_SLOT_BYTES,
                        bytemuck::bytes_of(&CasterUniformGpu {
                            clip_from_world: light.clip,
                            params: Vec4::new(seconds, frame.ambient_colour.w, 0.0, 0.0),
                            flags: UVec4::new(frame.flags.x, 0, 0, 0),
                            previous_clip_from_world: Mat4::IDENTITY,
                            previous_params: Vec4::ZERO,
                            local_light: light.position.extend(1.0),
                            padding: [Vec4::ZERO; 4],
                        }),
                    );
                }
            }

            if !settings.reflection_capture {
                state.indirect.set_quality(settings.quality);
                let signature = indirect_lighting_signature(
                    &frame,
                    state.local_lights.illumination_signature(),
                    settings.volumetric_clouds,
                    state
                        .post
                        .as_ref()
                        .map_or(0, |post| post.atmosphere_cache.environment_generation()),
                );
                state.indirect.prepare(
                    &queue,
                    &local.geometry,
                    &local.coverage,
                    &local.assets,
                    &local.tints,
                    inputs.camera,
                    dimension,
                    signature,
                );
            }
            let size = inputs.viewport;
            let effects = size.map(|value| {
                value
                    .div_ceil(crate::enhanced::quality::budget(settings.quality).effects_divisor)
                    .max(1)
            });
            let shafts_changed = if frame.flags.x & crate::enhanced::frame::FEATURE_SHAFTS == 0 {
                state.shafts.take().is_some()
            } else if state
                .shafts
                .as_ref()
                .is_none_or(|shafts| shafts.size != effects)
            {
                state.shafts = Some(crate::enhanced::targets::EffectTarget::new(
                    &device,
                    "enhanced light shafts",
                    effects,
                ));
                true
            } else {
                false
            };
            if shafts_changed && let Some(post) = &state.post {
                post.bindings.clear();
            }
            let scene_size = [
                target.main_texture().width(),
                target.main_texture().height(),
            ];
            if settings.reflection_capture {
                state.scene = None;
            } else if state
                .scene
                .as_ref()
                .is_none_or(|scene| scene.size != scene_size)
            {
                state.scene = Some(crate::enhanced::targets::SceneTargets::new(
                    &device,
                    scene_size,
                    target.main_texture_format(),
                ));
            }

            let own_shadow = state
                .shadow
                .as_ref()
                .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
            let shadow_view = capture_lighting
                .as_ref()
                .map_or(own_shadow, |lighting| &lighting.shadow);
            let own_point_shadow = state
                .point_shadow
                .as_ref()
                .map_or(&gpu.fallback_shadow, |shadow| &shadow.array);
            let point_shadow_view = capture_lighting
                .as_ref()
                .map_or(own_point_shadow, |lighting| &lighting.point_shadow);
            let shadow_frame = capture_lighting
                .as_ref()
                .map_or(&state.frame, |lighting| &lighting.frame);
            let light_buffer = capture_lighting
                .as_ref()
                .map_or(&state.local_lights.buffer, |lighting| &lighting.lights);
            let indirect_buffer = capture_lighting
                .as_ref()
                .map_or(&state.indirect.buffer, |lighting| &lighting.indirect);
            let own_cloud = state
                .post
                .as_ref()
                .map_or(&gpu.fallback_colour, |post| &post.cloud_shadow);
            let cloud_view = capture_lighting
                .as_ref()
                .map_or(own_cloud, |lighting| &lighting.cloud);
            let scene_colour = state
                .scene
                .as_ref()
                .map_or(&gpu.fallback_colour, |scene| &scene.colour_view);
            let scene_depth = state
                .scene
                .as_ref()
                .map_or(&gpu.fallback_depth, |scene| &scene.depth_view);
            let scene_motion = state
                .scene
                .as_ref()
                .map_or(&gpu.fallback_colour, |scene| &scene.motion_view);
            if !settings.reflection_capture
                && settings.shadows
                && lighting_ready
                && (state.shadow.is_some() || !state.local_lights.shadows.is_empty())
                && let Some(scene) = &state.scene
            {
                let half = scene_size.map(|value| value.div_ceil(2).max(1));
                if state
                    .local_shadow
                    .as_ref()
                    .is_none_or(|history| history.size != half)
                {
                    state.local_shadow = Some(
                        crate::enhanced::local_shadow_history::LocalShadowHistory::new(
                            &device, half,
                        ),
                    );
                }
                let history = state
                    .local_shadow
                    .as_mut()
                    .expect("local visibility allocated");
                history.set_caster_interval(if settings.waving && !actor_changed {
                    crate::enhanced::local_lights::POINT_SHADOW_ANIMATION_INTERVAL
                } else {
                    0.0
                });
                history.prepare(
                    &queue,
                    valid,
                    state.local_lights.shadow_lane_identities(),
                    time.delta_secs(),
                );
                history.sun.prepare(
                    &queue,
                    valid,
                    dimension,
                    local.sources.revision,
                    gpu.materials.id(),
                    time.delta_secs(),
                );
                history.bind(
                    &device,
                    &pipeline_cache,
                    &state.frame,
                    scene_depth,
                    &scene.receiver_normal_view,
                    &scene.motion_view,
                    point_shadow_view,
                    shadow_view,
                    &state.local_lights.buffer,
                    &gpu.linear_sampler,
                    &gpu.shadow_sampler,
                );
            } else {
                state.local_shadow = None;
            }
            let local_visibility = state
                .local_shadow
                .as_ref()
                .map_or(&gpu.fallback_colour, |history| &history.view);
            let local_metadata = state
                .local_shadow
                .as_ref()
                .map_or(&gpu.fallback_colour, |history| &history.metadata_view);
            let sun_visibility = state
                .local_shadow
                .as_ref()
                .map_or(&gpu.fallback_colour, |history| &history.sun.view);
            let binding_key = [
                shadow_view.id(),
                gpu.materials.id(),
                scene_colour.id(),
                scene_depth.id(),
                probe.array.id(),
                cloud_view.id(),
                state
                    .post
                    .as_ref()
                    .map_or(&gpu.fallback_colour, |post| &post.effects)
                    .id(),
                local_visibility.id(),
                scene_motion.id(),
                local_metadata.id(),
                point_shadow_view.id(),
                sun_visibility.id(),
            ];
            let buffer_key = (shadow_frame.id(), light_buffer.id(), indirect_buffer.id());
            if state.view_binding_key != Some(binding_key)
                || state.view_buffer_key != Some(buffer_key)
            {
                if let Some(post) = &state.post {
                    post.bindings.clear();
                }
                if !settings.reflection_capture {
                    state.indirect_bind_group = Some(
                        state.indirect.bind_group(
                            &device,
                            &pipeline_cache,
                            &state.frame,
                            &probe.array,
                            &gpu.linear_sampler,
                            &state.local_lights.buffer,
                            state
                                .post
                                .as_ref()
                                .map_or(&gpu.fallback_colour, |post| &post.cloud_shadow),
                        ),
                    );
                }
                state.view_bind_group = Some(
                    device.create_bind_group(
                        "enhanced view bind group",
                        &view_layout,
                        &[
                            BindGroupEntry {
                                binding: 0,
                                resource: state.frame.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource: BindingResource::TextureView(shadow_view),
                            },
                            BindGroupEntry {
                                binding: 2,
                                resource: BindingResource::Sampler(&gpu.shadow_sampler),
                            },
                            BindGroupEntry {
                                binding: 3,
                                resource: BindingResource::TextureView(&gpu.materials),
                            },
                            BindGroupEntry {
                                binding: 4,
                                resource: BindingResource::TextureView(scene_colour),
                            },
                            BindGroupEntry {
                                binding: 5,
                                resource: BindingResource::TextureView(scene_depth),
                            },
                            BindGroupEntry {
                                binding: 6,
                                resource: BindingResource::Sampler(&gpu.linear_sampler),
                            },
                            BindGroupEntry {
                                binding: 7,
                                resource: BindingResource::TextureView(&probe.array),
                            },
                            BindGroupEntry {
                                binding: 8,
                                resource: BindingResource::TextureView(cloud_view),
                            },
                            BindGroupEntry {
                                binding: 9,
                                resource: BindingResource::TextureView(
                                    state
                                        .post
                                        .as_ref()
                                        .map_or(&gpu.fallback_colour, |post| &post.effects),
                                ),
                            },
                            BindGroupEntry {
                                binding: 10,
                                resource: light_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 11,
                                resource: state.local_lights.tiles.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 12,
                                resource: indirect_buffer.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 16,
                                resource: BindingResource::TextureView(local_visibility),
                            },
                            BindGroupEntry {
                                binding: 17,
                                resource: BindingResource::TextureView(scene_motion),
                            },
                            BindGroupEntry {
                                binding: 18,
                                resource: shadow_frame.as_entire_binding(),
                            },
                            BindGroupEntry {
                                binding: 19,
                                resource: BindingResource::TextureView(local_metadata),
                            },
                            BindGroupEntry {
                                binding: 20,
                                resource: BindingResource::TextureView(point_shadow_view),
                            },
                            BindGroupEntry {
                                binding: 21,
                                resource: BindingResource::TextureView(sun_visibility),
                            },
                            BindGroupEntry {
                                binding: 22,
                                resource: BindingResource::TextureView(&local.scattering.view),
                            },
                            BindGroupEntry {
                                binding: 23,
                                resource: BindingResource::Sampler(&local.scattering.sampler),
                            },
                        ],
                    ),
                );
                state.view_binding_key = Some(binding_key);
                state.view_buffer_key = Some(buffer_key);
            }
            if !reflection_capture && state.caster_material_key != Some(gpu.materials.id()) {
                state.caster_bind_group = Some(device.create_bind_group(
                    "enhanced caster bind group",
                    &caster_layout,
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: &state.casters,
                                offset: 0,
                                size: NonZeroU64::new(CASTER_UNIFORM_BYTES),
                            }),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(&gpu.materials),
                        },
                    ],
                ));
                state.depth_bind_group = Some(device.create_bind_group(
                    "enhanced camera depth bind group",
                    &caster_layout,
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::Buffer(BufferBinding {
                                buffer: &state.depth_caster,
                                offset: 0,
                                size: NonZeroU64::new(CASTER_UNIFORM_BYTES),
                            }),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(&gpu.materials),
                        },
                    ],
                ));
                state.caster_material_key = Some(gpu.materials.id());
            }
            if !reflection_capture
                && Some(view.retained_view_entity.main_entity.id()) == probe_origin.source
            {
                use std::hash::{Hash, Hasher};
                let mut signature = std::collections::hash_map::DefaultHasher::new();
                state
                    .local_lights
                    .illumination_signature()
                    .hash(&mut signature);
                state.local_lights.shadow_identity().hash(&mut signature);
                own_shadow.id().hash(&mut signature);
                own_point_shadow.id().hash(&mut signature);
                settings.shadows.hash(&mut signature);
                (frame.flags.x & crate::enhanced::frame::FEATURE_SHADOWS != 0).hash(&mut signature);
                for value in frame.camera_time.truncate().to_array() {
                    ((value / 4.0).floor() as i32).hash(&mut signature);
                }
                probe_origin.publish_lighting_signature(signature.finish());
            }
        }
    }
}

fn indirect_lighting_signature(
    frame: &crate::enhanced::frame::EnhancedFrameGpu,
    local_lights: u64,
    clouds: bool,
    environment_generation: u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut signature = std::collections::hash_map::DefaultHasher::new();
    for value in frame
        .light_direction
        .to_array()
        .into_iter()
        .chain(frame.light_colour.to_array())
        .chain(frame.ambient_colour.to_array())
        .chain(frame.atmosphere.to_array().into_iter().take(1))
    {
        ((value * 128.0).round() as i32).hash(&mut signature);
    }
    if frame.atmosphere.x < 0.5 {
        for value in frame
            .sky_horizon
            .to_array()
            .into_iter()
            .chain(frame.sky_zenith.to_array())
        {
            ((value * 128.0).round() as i32).hash(&mut signature);
        }
    }
    local_lights.hash(&mut signature);
    environment_generation.hash(&mut signature);
    clouds.hash(&mut signature);
    if clouds {
        (frame.cloud_shadow.w > 0.5).hash(&mut signature);
        for value in [
            frame.clouds.x * 128.0,
            frame.clouds.y,
            frame.clouds.z,
            frame.clouds.w,
        ] {
            (value.round() as i32).hash(&mut signature);
        }
    }
    signature.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_light_dimension_needs_no_diagnostic_publication_witness() {
        assert_eq!(render_dimension(None), None);
        for dimension in [-42, 0, 700] {
            let environment = crate::AtmosphereViewInputs {
                dimension,
                ..default()
            };
            assert_eq!(render_dimension(Some(&environment)), Some(dimension));
        }
    }
    #[test]
    fn cloud_shadow_sampling_survives_temporal_history_resets() {
        let mut settings = EnhancedRendering::default();
        assert!(cloud_shadow_sampling_enabled(&settings, true));
        settings.temporal_aa = false;
        assert!(cloud_shadow_sampling_enabled(&settings, true));
        assert!(!cloud_shadow_sampling_enabled(&settings, false));
        settings.volumetric_clouds = false;
        assert!(!cloud_shadow_sampling_enabled(&settings, true));
    }
    #[test]
    fn indirect_relighting_tracks_source_readiness_and_palette_without_frame_churn() {
        use bytemuck::Zeroable;
        let mut frame = crate::enhanced::frame::EnhancedFrameGpu::zeroed();
        frame.atmosphere.x = 1.0;
        let cold = indirect_lighting_signature(&frame, 7, true, 0);
        frame.cloud_shadow.w = 1.0;
        let ready = indirect_lighting_signature(&frame, 7, true, 0);
        assert_ne!(cold, ready);
        frame.camera_time.w += 10.0;
        frame.temporal.x += 1.0;
        frame.clouds.w += 0.01;
        assert_eq!(ready, indirect_lighting_signature(&frame, 7, true, 0));
        assert_ne!(ready, indirect_lighting_signature(&frame, 7, true, 1));
        frame.atmosphere.x = 0.0;
        let fallback = indirect_lighting_signature(&frame, 7, true, 0);
        frame.sky_horizon.x += 0.1;
        assert_ne!(fallback, indirect_lighting_signature(&frame, 7, true, 0));
        let fallback = indirect_lighting_signature(&frame, 7, true, 0);
        frame.sky_zenith.z += 0.1;
        assert_ne!(fallback, indirect_lighting_signature(&frame, 7, true, 0));
        assert_ne!(
            indirect_lighting_signature(&frame, 7, true, 0),
            indirect_lighting_signature(&frame, 8, true, 0)
        );
    }
}
