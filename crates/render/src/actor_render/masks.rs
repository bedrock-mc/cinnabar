//! Bounded owner-only actor coverage, using the scene's exact meshes, poses and cutouts.

use super::*;
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{SubApp, SystemSet},
    render::{
        render_resource::RenderPassDepthStencilAttachment,
        renderer::{RenderContext, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};
use mod_api::drawing::{
    DrawingHandle, MAX_MASK_ATTACHMENT_BYTES, MAX_MASK_VERTICES, MaskDepth, MaskEntity,
};
use std::collections::{BTreeMap, HashMap};

pub(crate) const FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    actor: ActorPipelineKey,
    depth: MaskDepth,
}

impl SpecializerKey for Key {
    const IS_CANONICAL: bool = false;
    type Canonical = (ActorPipelineContract, MaskDepth);
}

struct MaskSpecializer;

/// Binds only the private companion depth, with the view's exact sample count.
fn dissolve_layout(multisampled: bool) -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "mask dissolve depth",
        &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        }],
    )
}

/// Identifies the depth-only companion independently of authored material flags.
fn dissolve_depth(material: u32) -> bool {
    material & assets::EntityRenderMaterialState::KIND_MASK
        == assets::EntityRenderMaterial::DissolveDepth as u32
}

impl Specializer<RenderPipeline> for MaskSpecializer {
    type Key = Key;

    /// Preserves scene coverage and culling while replacing its colour and depth-write policy.
    fn specialize(
        &self,
        key: Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Key>, BevyError> {
        let contract = ActorPipelineSpecializer.specialize(key.actor, descriptor)?;
        if dissolve_depth(key.actor.material) {
            descriptor
                .depth_stencil
                .as_mut()
                .unwrap()
                .depth_write_enabled = Some(true);
            descriptor.fragment.as_mut().unwrap().targets.clear();
            return Ok((contract, key.depth));
        }
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        depth.depth_write_enabled = Some(false);
        depth.depth_compare = Some(if key.depth == MaskDepth::Always {
            CompareFunction::Always
        } else {
            CompareFunction::GreaterEqual
        });
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.entry_point = Some("actor_mask_fragment".into());
        let dissolve = key.actor.material & assets::EntityRenderMaterialState::KIND_MASK
            == assets::EntityRenderMaterial::DissolveColor as u32;
        if dissolve {
            descriptor
                .layout
                .push(dissolve_layout(key.actor.msaa.samples() > 1));
            fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
                "MASK_DISSOLVE".into(),
                true,
            ));
            fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
                "MASK_DISSOLVE_MSAA".into(),
                key.actor.msaa.samples() > 1,
            ));
        }
        fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
            "MASK_BLEND".into(),
            crate::actor::material::state(key.actor.material).is_some_and(|state| state.blend),
        ));
        let component = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        };
        fragment.targets[0] = Some(ColorTargetState {
            format: FORMAT,
            blend: Some(wgpu::BlendState {
                color: component,
                alpha: component,
            }),
            write_mask: ColorWrites::ALL,
        });
        Ok((contract, key.depth))
    }
}

/// One view and one owner's immutable-depth mask; every frame clears it before sampling.
pub(crate) struct MaskTarget {
    texture: Texture,
    render_view: TextureView,
    _resolved: Option<Texture>,
    pub(crate) view: TextureView,
    dissolve_view: TextureView,
    dissolve_binding: Option<BindGroup>,
    draws: Vec<Vec<(crate::actor::gpu::ActorDrawSpan, CachedRenderPipelineId)>>,
    frame_generation: u64,
    ready: bool,
}

impl MaskTarget {
    /// Accounts for all samples and the resolve attachment before allocating either texture.
    fn new(device: &RenderDevice, depth: &ViewDepthStencilTexture) -> Option<Self> {
        let size = depth.texture().size();
        let samples = depth.texture().sample_count();
        let bytes = mod_render::masks::attachment_bytes(size.width, size.height, samples)?;
        let allocate = |samples| {
            device.create_texture(&TextureDescriptor {
                label: Some("owner actor mask"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: TextureDimension::D2,
                format: FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let (texture, resolved, dissolve) =
            gpu_allocation::checked(device, "owner actor mask", bytes, || {
                (
                    allocate(samples),
                    (samples > 1).then(|| allocate(1)),
                    device.create_texture(&TextureDescriptor {
                        label: Some("private mask dissolve depth"),
                        size,
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: TextureDimension::D2,
                        format: CORE_3D_DEPTH_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    }),
                )
            })?;
        let render_view = texture.create_view(&default());
        let view = resolved.as_ref().map_or_else(
            || render_view.clone(),
            |texture| texture.create_view(&default()),
        );
        Some(Self {
            texture,
            render_view,
            _resolved: resolved,
            view,
            dissolve_view: dissolve.create_view(&default()),
            dissolve_binding: None,
            draws: Vec::new(),
            frame_generation: 0,
            ready: false,
        })
    }

    /// Reallocates on physical size or sample-count changes, never by logical UI dimensions.
    fn matches(&self, depth: &ViewDepthStencilTexture) -> bool {
        self.texture.size() == depth.texture().size()
            && self.texture.sample_count() == depth.texture().sample_count()
    }
}

#[derive(Resource)]
pub(crate) struct MaskGpu {
    variants: Variants<RenderPipeline, MaskSpecializer>,
    pub(crate) targets: HashMap<(Entity, DrawingHandle), MaskTarget>,
}

impl FromWorld for MaskGpu {
    /// Shares actor bind layouts and shader ownership; no new mesh buffers are allocated.
    fn from_world(_: &mut World) -> Self {
        Self {
            variants: Variants::new(
                MaskSpecializer,
                actor_pipeline_descriptor(actor_bind_group_layout()),
            ),
            targets: HashMap::new(),
        }
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MaskLabel;

/// Installs preparation once; the post-pass owner installs the per-view draw schedule.
pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<MaskGpu>().add_systems(
        Render,
        prepare
            .in_set(RenderSystems::PrepareBindGroups)
            .after(prepare_actor_bind_group),
    );
    install_graph(app.world_mut());
}

#[derive(Resource)]
struct Installed;

/// Draws masks after the world and hand, before any mod post effect or UI consumes them.
pub(crate) fn install_graph(world: &mut World) {
    if world.contains_resource::<Installed>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                draw.in_set(MaskLabel)
                    .after(Core3dSystems::PostProcess)
                    .before(crate::mod_render::ModPassLabel)
                    .before(crate::ui_render::UiOverlayLabel)
                    .before(crate::ui_render::overlay::UiOverlayPostLabel),
            );
        })
        .is_ok()
    {
        world.insert_resource(Installed);
    }
}

/// Matches every layer of one actor incarnation; runtime ID reuse cannot inherit membership.
fn selected(mask: &mod_render::masks::Mask, identity: &render_model::ActorRenderIdentity) -> bool {
    mask.authorized()
        && mask.entities.contains(&MaskEntity {
            connection: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision,
        })
}

/// Selects only admitted draw spans, splitting shared spans at authorized entity boundaries.
fn selection(
    mask: &mod_render::masks::Mask,
    gpu: &ActorGpu,
) -> Option<Vec<crate::actor::gpu::ActorDrawSpan>> {
    if gpu.instance_count == 0 {
        return None;
    }
    let mut draws = Vec::new();
    let mut vertices = 0u64;
    for span in &gpu.spans {
        for first in span.first..span.first.checked_add(span.count)? {
            let entry = gpu.manifest.get(first as usize)?;
            if !selected(mask, &entry.identity) {
                continue;
            }
            vertices = vertices.checked_add(u64::from(span.vertex_count))?;
            if vertices > MAX_MASK_VERTICES {
                return None;
            }
            draws.push(crate::actor::gpu::ActorDrawSpan {
                first,
                count: 1,
                ..*span
            });
        }
    }
    Some(draws)
}

/// Reclaims revoked owners/views before charging a complete frame's attachments.
fn prepare(
    scene: Option<Res<render_scene::ModRenderScene>>,
    gpu: Option<Res<ActorGpu>>,
    mut masks: ResMut<MaskGpu>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    views: Query<(Entity, &Msaa, &ViewDepthStencilTexture, &ViewTarget)>,
) {
    let (Some(scene), Some(gpu)) = (scene, gpu) else {
        masks.targets.clear();
        return;
    };
    let active: Vec<_> = scene
        .masks
        .iter()
        .filter(|mask| mask.authorized() && !mask.entities.is_empty())
        .collect();
    // Admit a whole frame or none: load order cannot consume another owner's attachment budget.
    let bytes = views.iter().try_fold(0u64, |sum, (_, _, depth, _)| {
        let size = depth.texture().size();
        sum.checked_add(
            mod_render::masks::attachment_bytes(
                size.width,
                size.height,
                depth.texture().sample_count(),
            )?
            .checked_mul(active.len() as u64)?,
        )
    });
    if bytes.is_none_or(|bytes| bytes > MAX_MASK_ATTACHMENT_BYTES) {
        masks.targets.clear();
        return;
    }
    masks.targets.retain(|(view, handle), _| {
        views.contains(*view) && active.iter().any(|mask| mask.handle == *handle)
    });
    for (entity, msaa, depth, _) in &views {
        for mask in &active {
            let key = (entity, mask.handle);
            if masks
                .targets
                .get(&key)
                .is_none_or(|target| !target.matches(depth))
            {
                masks.targets.remove(&key);
                let Some(target) = MaskTarget::new(&device, depth) else {
                    continue;
                };
                masks.targets.insert(key, target);
            }
            let spans = selection(mask, &gpu);
            let mut draws = BTreeMap::<_, Vec<_>>::new();
            let mut ready = spans.is_some();
            for span in spans.into_iter().flatten() {
                let actor = ActorPipelineKey {
                    msaa: *msaa,
                    hdr: false,
                    enhanced: false,
                    material: span.material,
                };
                let Ok(id) = masks.variants.specialize(
                    &cache,
                    Key {
                        actor,
                        depth: mask.depth,
                    },
                ) else {
                    ready = false;
                    continue;
                };
                ready &= cache.get_render_pipeline(id).is_some();
                ready &= if span.page == 0 {
                    gpu.bind_group.is_some()
                } else {
                    gpu.artwork_current
                        && gpu
                            .artwork
                            .pages
                            .get(usize::from(span.page) - 1)
                            .is_some_and(|page| page.bind_group.is_some())
                };
                let kind = span.material & assets::EntityRenderMaterialState::KIND_MASK;
                let companion = dissolve_depth(span.material)
                    || kind == assets::EntityRenderMaterial::DissolveColor as u32;
                let owner = companion.then(|| render_model::ActorRenderIdentity {
                    layer: 0,
                    ..gpu.manifest[span.first as usize].identity
                });
                draws.entry(owner).or_default().push((span, id));
            }
            let target = masks.targets.get_mut(&key).unwrap();
            if target.dissolve_binding.is_none() {
                target.dissolve_binding =
                    gpu_allocation::checked(&device, "mask dissolve binding", 0, || {
                        device.create_bind_group(
                            "mask dissolve binding",
                            &cache.get_bind_group_layout(&dissolve_layout(msaa.samples() > 1)),
                            &[BindGroupEntry {
                                binding: 0,
                                resource: BindingResource::TextureView(&target.dissolve_view),
                            }],
                        )
                    });
            }
            target.draws = draws.into_values().collect();
            ready &= target.dissolve_binding.is_some();
            target.ready = ready;
            target.frame_generation = gpu.frame_generation;
        }
    }
}

/// Clears even an empty or unavailable selection so a previous silhouette is never reused.
fn draw(
    world: &World,
    query: ViewQuery<(
        &ViewDepthStencilTexture,
        &ExtractedView,
        Option<&bevy::render::camera::ExtractedCamera>,
    )>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let entity = query.entity();
    let (depth, view, camera) = query.into_inner();
    let depth_range = camera
        .and_then(|camera| camera.viewport.as_ref())
        .map_or(0.0..1.0, |viewport| viewport.depth.clone());
    let (Some(masks), Some(gpu), Some(lightmap), Some(offset), Some(cache)) = (
        world.get_resource::<MaskGpu>(),
        world.get_resource::<ActorGpu>(),
        world.get_resource::<crate::lighting::LightmapGpu>(),
        world.get::<ViewUniformOffset>(entity),
        world.get_resource::<PipelineCache>(),
    ) else {
        return Ok(());
    };
    for ((owner_view, _), target) in &masks.targets {
        if *owner_view != entity {
            continue;
        }
        let attachments = [Some(RenderPassColorAttachment {
            view: &target.render_view,
            depth_slice: None,
            resolve_target: (target.texture.sample_count() > 1).then_some(&target.view),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        // Clear the union once; each actor's companion gets independent private depth.
        drop(context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("clear owner actor coverage"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        if !target.ready || target.frame_generation != gpu.frame_generation {
            continue;
        }
        let Some(rect) = crate::render_bounds::scissor(
            render_model::UiScissor::new(
                view.viewport.x,
                view.viewport.y,
                view.viewport.z,
                view.viewport.w,
            ),
            crate::render_bounds::extent(&target.render_view),
        ) else {
            continue;
        };
        for draws in &target.draws {
            let paired = draws.iter().any(|(span, _)| {
                let kind = span.material & assets::EntityRenderMaterialState::KIND_MASK;
                dissolve_depth(span.material)
                    || kind == assets::EntityRenderMaterial::DissolveColor as u32
            });
            for prepass in [true, false] {
                if prepass && !paired {
                    continue;
                }
                let mut attachment = depth.get_attachment(wgpu::StoreOp::Store);
                attachment.depth_ops = Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                });
                let private_depth = RenderPassDepthStencilAttachment {
                    view: &target.dissolve_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                };
                let attachments = [Some(RenderPassColorAttachment {
                    view: &target.render_view,
                    depth_slice: None,
                    resolve_target: (target.texture.sample_count() > 1).then_some(&target.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })];
                let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    label: Some(if prepass {
                        "private mask dissolve depth"
                    } else {
                        "owner actor coverage"
                    }),
                    color_attachments: if prepass { &[] } else { &attachments },
                    depth_stencil_attachment: Some(if prepass {
                        private_depth
                    } else {
                        attachment
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                    depth_range.start,
                    depth_range.end,
                );
                pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
                pass.set_bind_group(1, &lightmap.bind_group, &[]);
                for (span, id) in draws {
                    if dissolve_depth(span.material) != prepass {
                        continue;
                    }
                    let Some(pipeline) = cache.get_render_pipeline(*id) else {
                        continue;
                    };
                    let binding = if span.page == 0 {
                        gpu.bind_group.as_ref()
                    } else {
                        gpu.artwork
                            .pages
                            .get(usize::from(span.page) - 1)
                            .and_then(|page| page.bind_group.as_ref())
                    };
                    let Some(binding) = binding else {
                        continue;
                    };
                    pass.set_render_pipeline(pipeline);
                    pass.set_bind_group(0, binding, &[offset.offset]);
                    if !prepass && paired {
                        pass.set_bind_group(2, target.dissolve_binding.as_ref().unwrap(), &[]);
                    }
                    pass.draw(0..span.vertex_count, span.first..span.first + span.count);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
