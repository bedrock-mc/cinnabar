use super::*;

/// Retains the overlap stencil and bindings for this view's shadow coverage.
#[derive(Component)]
pub(crate) struct EntityShadowView {
    pub(super) stencil: Texture,
    pub(super) stencil_view: TextureView,
    pub(super) format: TextureFormat,
    pub(super) pipeline: CachedRenderPipelineId,
    pub(super) rect: Option<[u32; 4]>,
    pub(super) bind_group: Option<(BindGroupKey, BindGroup)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct BindGroupKey {
    pub(super) depth: TextureViewId,
    pub(super) instances: BufferId,
    pub(super) view_uniforms: BufferId,
}

type ShadowViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static Msaa,
        Option<&'static mut EntityShadowView>,
    ),
    With<Camera3d>,
>;

/// Allocates a stencil only for views containing visible shadow volumes.
pub(crate) fn prepare_shadow_views(
    mut commands: Commands,
    scene: Res<EntityShadowScene>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    mut gpu: ResMut<EntityShadowGpu>,
    mut views: ShadowViews,
) {
    for (entity, view, target, msaa, state) in &mut views {
        let clip_from_world = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
        let rect = shadow_screen_rect(clip_from_world, &scene.0.shadows, view.viewport.to_array());
        let main = target.main_texture();
        let format = main.format().remove_srgb_suffix();
        if let Some(mut state) = state {
            state.rect = rect;
            if rect.is_none()
                || (state.stencil.size() == main.size()
                    && state.format == format
                    && state.stencil.sample_count() == msaa.samples())
            {
                continue;
            }
        }
        if rect.is_none() {
            continue;
        }
        let stencil = device.create_texture(&TextureDescriptor {
            label: Some("entity shadow overlap stencil"),
            size: main.size(),
            mip_level_count: 1,
            sample_count: msaa.samples(),
            dimension: TextureDimension::D2,
            format: TextureFormat::Stencil8,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let stencil_view = stencil.create_view(&Default::default());
        let pipeline = gpu.pipeline(&cache, format, msaa.samples());
        commands.entity(entity).insert(EntityShadowView {
            stencil,
            stencil_view,
            format,
            pipeline,
            rect,
            bind_group: None,
        });
    }
}

/// Rebinds only when depth or uploaded buffer storage changes.
pub(crate) fn prepare_shadow_bind_groups(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    gpu: Res<EntityShadowGpu>,
    view_uniforms: Res<ViewUniforms>,
    mut views: Query<(&mut EntityShadowView, &ViewDepthTexture)>,
) {
    let (Some(binding), Some(uniforms)) = (
        view_uniforms.uniforms.binding(),
        view_uniforms.uniforms.buffer(),
    ) else {
        return;
    };
    for (mut state, depth) in &mut views {
        if state.rect.is_none() || !depth_is_sampleable(depth) {
            continue;
        }
        let key = BindGroupKey {
            depth: depth.view().id(),
            instances: gpu.instances.id(),
            view_uniforms: uniforms.id(),
        };
        if state
            .bind_group
            .as_ref()
            .is_some_and(|(known, _)| *known == key)
        {
            continue;
        }
        let bind_group = device.create_bind_group(
            "entity shadow bind group",
            &cache
                .get_bind_group_layout(&gpu.layouts[usize::from(state.stencil.sample_count() > 1)]),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: binding.clone(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(depth.view()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: gpu.instances.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: gpu.params.as_entire_binding(),
                },
            ],
        );
        state.bind_group = Some((key, bind_group));
    }
}

/// Shadows read the original depth samples without an intermediate resolve.
fn depth_is_sampleable(depth: &ViewDepthTexture) -> bool {
    depth
        .texture
        .usage()
        .contains(TextureUsages::TEXTURE_BINDING)
}
