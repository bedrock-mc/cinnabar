//! Records a frame of a pack: clears, the shadow pass, `prepare*`, the sky and opaque
//! geometry (or the Vanilla scene copy into `colortex0` and `depthtex*` for packs without
//! terrain programs), `deferred*`, translucent geometry, `composite*` (with compute
//! variants), `final` into the screen, the copy-back (or exchange of textures) of buffers
//! that end flipped, and the screen's presentation into the view.

use super::frame_geometry::{
    FrameGeometry, bind_geometry, record_opaque, record_shadow, record_translucent,
};
use super::loader::{PackGpu, ProgramPipeline};
use super::plan::PassStep;
use super::targets::{Groups, ViewBindings, ViewTargets, is_compute};
use super::uniforms::{
    DrawUniforms, FAR_PLANE_SCALE, FrameUniforms, GL_NEAR, HostValue, pack_block,
};
use crate::gpu_timing::{GpuTimestamps, within_stage};
use bevy::math::Vec3;
use render_scene::RuntimeStage;
use shaderpack::{CustomUniforms, ProgramGroup};
use std::collections::BTreeSet;
use std::sync::Arc;
use wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView};

/// Per-view values the geometry draws need beyond the frame's uniforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawContext {
    /// The terrain origin relative to the camera (terrain draws' `chunkOffset`).
    pub chunk_offset: Vec3,
    /// The block atlas's texel size (terrain draws' `atlasSize`).
    pub atlas_size: [i32; 2],
}

/// One view's resources and per-frame state for a loaded pack.
pub(crate) struct ViewState {
    pub pack: Arc<PackGpu>,
    pub targets: ViewTargets,
    pub bindings: ViewBindings,
    /// View-local scene conversion planes and source rectangle.
    pub(super) depth_parameters: wgpu::Buffer,
    pub history: super::uniforms::UniformHistory,
    custom: CustomUniforms,
    scratch: Vec<u8>,
    unknown: BTreeSet<String>,
    unknown_logged: bool,
    /// The developer uniform dump, when [`super::uniform_audit::DUMP_ENV`] names a file.
    audit: Option<super::uniform_audit::UniformAudit>,
    /// Programs and draws already reported as skipped.
    pub skipped: BTreeSet<String>,
    /// The atlas generation the draw bind groups were built over (`None` inside: the
    /// placeholder); `None` before the first frame.
    pub atlas_generation: Option<Option<u64>>,
    /// Whether a frame has seen the player's biome: see [`Self::enter_world`].
    in_world: bool,
}

impl ViewState {
    /// Prepares a view, leaving the native scene available when resource creation fails.
    pub(crate) fn new(
        device: &Device,
        pack: Arc<PackGpu>,
        screen: [u32; 2],
    ) -> Result<Self, String> {
        let mut targets = ViewTargets::new(device, &pack, screen)?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
        let bindings =
            crate::gpu_allocation::checked(&render_device, "shader pack view bindings", 0, || {
                ViewBindings::new(device, &pack, &mut targets)
            });
        if let Some(error) = bevy::tasks::block_on(scope.pop()) {
            return Err(error.to_string());
        }
        let bindings =
            bindings.ok_or_else(|| "shader pack view bindings ran out of GPU memory".to_owned())?;
        let (custom, errors) = CustomUniforms::new(&pack.custom_uniforms);
        for error in errors {
            bevy::log::debug!(
                target: "cinnabar::shaderpack",
                pack = %pack.name, line = error.line, name = %error.name,
                "custom uniform dropped: {}", error.message
            );
        }
        Ok(Self {
            depth_parameters: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader pack view depth parameters"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            pack,
            targets,
            bindings,
            history: Default::default(),
            custom,
            scratch: vec![],
            unknown: BTreeSet::new(),
            unknown_logged: false,
            audit: super::uniform_audit::UniformAudit::from_env(),
            skipped: BTreeSet::new(),
            atlas_generation: None,
            in_world: false,
        })
    }

    /// Whether the state still matches `pack` and the screen size.
    pub(crate) fn matches(&self, pack: &Arc<PackGpu>, screen: [u32; 2]) -> bool {
        Arc::ptr_eq(&self.pack, pack) && self.targets.screen_size == screen
    }

    /// Keeps native queues available when a geometry draw would reject its attachments.
    pub(crate) fn geometry_targets_ready(&self) -> bool {
        self.pack.draws.iter().all(|draw| {
            let shadow = self.pack.shadow.as_ref().filter(|_| draw.kind.shadow());
            let mut size = draw.kind.depth().then(|| {
                shadow.map_or(self.targets.screen_size, |shadow| {
                    [shadow.slots.resolution; 2]
                })
            });
            draw.outputs.iter().flatten().all(|index| {
                let extent = if let Some(shadow) = shadow {
                    shadow
                        .slots
                        .color
                        .iter()
                        .find(|(buffer, ..)| buffer == index)
                        .map(|_| [shadow.slots.resolution; 2])
                } else {
                    self.targets
                        .buffers
                        .get(*index as usize)
                        .and_then(|buffer| buffer.as_ref())
                        .map(|buffer| buffer.size)
                };
                extent.is_some_and(|extent| super::frame_geometry::admit_extent(&mut size, extent))
            })
        })
    }

    /// Restarts every smoothed uniform (`smooth()` custom uniforms, `wetness`,
    /// `eyeBrightnessSmooth`, `centerDepthSmooth`) on the first frame that knows the player's
    /// biome. Iris builds a pack's uniforms once the world renders, so they start from world
    /// values; frames before that (joining, terrain arriving) read placeholder biome values,
    /// and smoothing from them would hold Photon's 1000 s `biome_temperature` near the
    /// placeholder for the whole session.
    pub(crate) fn enter_world(&mut self, biome_known: bool) {
        if biome_known && !self.in_world {
            self.in_world = true;
            self.custom.restart_smoothing();
            self.history.started = false;
        }
    }

    /// Evaluates custom uniforms and writes every program's and geometry draw's uniform
    /// block.
    pub(crate) fn write_uniforms(
        &mut self,
        queue: &Queue,
        frame: &FrameUniforms,
        frame_time: f32,
        context: &DrawContext,
    ) {
        let builtin = |name: &str| -> Option<shaderpack::UniformValue> {
            if let Some(value) = frame.value(name) {
                return value.to_custom();
            }
            // `gbufferModelView.3.1`: a matrix element (column, row).
            let (matrix, rest) = name.split_once('.')?;
            let (column, row) = rest.split_once('.')?;
            let (column, row): (usize, usize) = (column.parse().ok()?, row.parse().ok()?);
            match frame.value(matrix)? {
                HostValue::Mat4(m) if column < 4 && row < 4 => {
                    Some(shaderpack::UniformValue::Float(m.col(column)[row]))
                }
                HostValue::Mat3(m) if column < 3 && row < 3 => {
                    Some(shaderpack::UniformValue::Float(m.col(column)[row]))
                }
                _ => None,
            }
        };
        let auditing = self
            .audit
            .as_mut()
            .is_some_and(|audit| audit.begin(&self.pack.name, self.history.frame));
        let missing_inputs: Vec<String> = if auditing {
            self.custom
                .inputs()
                .filter(|name| builtin(name).is_none())
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        };
        let custom = self.custom.evaluate(&builtin, frame_time);
        let lookup = |name: &str| {
            custom
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| HostValue::from_custom(*v))
                .or_else(|| frame.value(name))
        };
        for (program, buffer) in self.pack.programs.iter().zip(&self.bindings.uniforms) {
            let (Some(block), Some(buffer)) = (&program.reflection.uniforms, buffer) else {
                continue;
            };
            pack_block(block, &lookup, &mut self.scratch, &mut self.unknown);
            queue.write_buffer(buffer, 0, &self.scratch);
            if let Some(audit) = &mut self.audit {
                audit.block(&format!("program {}", program.name), block, &lookup);
            }
        }
        for (draw, binding) in self.pack.draws.iter().zip(&self.bindings.draws) {
            let program = &self.pack.programs[draw.program];
            let (Some(block), Some(buffer)) = (&program.reflection.uniforms, &binding.uniforms)
            else {
                continue;
            };
            let (model_view, projection) = if draw.kind.shadow() {
                (frame.shadow_model_view(), frame.shadow_projection())
            } else {
                (frame.model_view(), frame.projection())
            };
            let terrain = draw.kind.category().is_some();
            let values = DrawUniforms {
                model_view,
                projection,
                chunk_offset: if terrain {
                    context.chunk_offset
                } else {
                    Vec3::ZERO
                },
                render_stage: draw.kind.render_stage(),
                lightmap_matrix: draw.kind.lightmap_matrix(),
                atlas_size: if terrain { context.atlas_size } else { [0; 2] },
                entity_id: -1,
            };
            let lookup = |name: &str| values.value(name).or_else(|| lookup(name));
            pack_block(block, &lookup, &mut self.scratch, &mut self.unknown);
            queue.write_buffer(buffer, 0, &self.scratch);
            if let Some(audit) = &mut self.audit {
                audit.block(
                    &format!("draw {:?} with {}", draw.kind, program.name),
                    block,
                    &lookup,
                );
            }
        }
        if let Some(audit) = &mut self.audit {
            audit.finish(custom, missing_inputs.iter().map(String::as_str));
        }
        if !self.unknown_logged && !self.unknown.is_empty() {
            self.unknown_logged = true;
            bevy::log::debug!(
                target: "cinnabar::shaderpack",
                pack = %self.pack.name, count = self.unknown.len(),
                "uniforms without a host value are zero: {:?}", self.unknown
            );
        }
    }
}

/// What a view hands the pack for one frame.
pub(crate) struct FrameIo<'a> {
    /// Physical source and destination rectangle: x, y, width, height.
    pub viewport: [u32; 4],
    /// The finished scene colour, sampled as linear values.
    pub scene_colour: &'a TextureView,
    /// The scene depth (infinite reverse-Z).
    pub scene_depth: &'a TextureView,
    /// Near plane of the scene projection.
    pub scene_near: f32,
    /// `1 /` the scene projection's x and y scales.
    pub inverse_focal: [f32; 2],
    /// `far`: the render distance in blocks.
    pub render_distance: f32,
    /// `fogColor`, which `colortex0` clears to by default.
    pub fog_color: [f32; 3],
    /// Where the presented frame goes, and its format.
    pub destination: &'a TextureView,
    pub destination_format: TextureFormat,
    /// Terrain, atlas and sky for the pack's geometry programs; without it (or without
    /// terrain programs) the Vanilla scene is copied into `colortex0` instead.
    pub geometry: Option<FrameGeometry<'a>>,
    /// GPU timestamps, which time the pack's pass groups during aggregate profiling.
    pub timestamps: Option<&'a GpuTimestamps>,
}

pub(crate) fn clear_attachment(
    view: &TextureView,
    colour: [f64; 4],
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: colour[0],
                g: colour[1],
                b: colour[2],
                a: colour[3],
            }),
            store: wgpu::StoreOp::Store,
        },
    })
}

pub(crate) fn load_attachment(view: &TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        },
    })
}

pub(crate) fn pass<'a>(
    encoder: &'a mut CommandEncoder,
    label: &str,
    attachments: &[Option<wgpu::RenderPassColorAttachment<'_>>],
    depth: Option<wgpu::RenderPassDepthStencilAttachment<'_>>,
) -> wgpu::RenderPass<'a> {
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: attachments,
            depth_stencil_attachment: depth,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
        .forget_lifetime()
}

/// Records the whole pack frame. Uniforms must have been written for this frame.
pub(crate) fn record_frame(
    device: &Device,
    queue: &Queue,
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    io: &FrameIo<'_>,
) {
    let pack = Arc::clone(&state.pack);
    write_depth_parameters(queue, &state.depth_parameters, io);
    for plan in &pack.buffers {
        let (Some(clear), Some(buffer)) = (
            plan.clear,
            state.targets.buffers[plan.index as usize].as_ref(),
        ) else {
            continue;
        };
        let colour = clear.resolve(io.fog_color);
        for view in &buffer.attachment {
            pass(
                encoder,
                "shader pack clear",
                &[clear_attachment(view, colour)],
                None,
            );
        }
    }
    let geometry = io.geometry.as_ref().filter(|_| !pack.draws.is_empty());
    let timestamps = io.timestamps;
    if let Some(geometry) = geometry {
        bind_geometry(device, state, geometry);
        within_stage(
            timestamps,
            RuntimeStage::GpuShaderpackShadow,
            encoder,
            |encoder| {
                record_shadow(device, encoder, state, geometry);
                let shadow_size = pack
                    .shadow
                    .as_ref()
                    .map_or(state.targets.screen_size, |s| [s.slots.resolution; 2]);
                run_steps(encoder, state, Steps::Shadowcomp, shadow_size);
            },
        );
    }
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackOpaque,
        encoder,
        |encoder| {
            run_steps(encoder, state, Steps::Prepare, state.targets.screen_size);
            match geometry {
                Some(geometry) => {
                    record_opaque(device, queue, encoder, state, geometry, timestamps);
                }
                None => copy_scene(
                    device,
                    encoder,
                    &pack,
                    &state.targets,
                    &state.depth_parameters,
                    io,
                ),
            }
        },
    );
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackDeferred,
        encoder,
        |encoder| run_steps(encoder, state, Steps::Deferred, state.targets.screen_size),
    );
    if let Some(geometry) = geometry {
        within_stage(
            timestamps,
            RuntimeStage::GpuShaderpackTranslucent,
            encoder,
            |encoder| {
                record_translucent(
                    device,
                    encoder,
                    state,
                    geometry,
                    (io.scene_colour, io.scene_depth),
                );
            },
        );
    }
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackComposite,
        encoder,
        |encoder| record_composite(device, encoder, state, io),
    );
}

/// `composite*` and `final`, the copy-back of buffers that end flipped (or the exchange of
/// their textures), and the screen's presentation into the view.
fn record_composite(
    device: &Device,
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    io: &FrameIo<'_>,
) {
    let pack = Arc::clone(&state.pack);
    run_steps(encoder, state, Steps::Composite, state.targets.screen_size);
    let targets = &state.targets;
    for &index in &pack.frame.swap_back {
        let Some(buffer) = targets.buffers[index as usize].as_ref() else {
            continue;
        };
        let main = buffer.texture(0);
        encoder.copy_texture_to_texture(
            buffer.texture(buffer.side(true)).as_image_copy(),
            main.as_image_copy(),
            main.size(),
        );
    }
    state.targets.swap_sides(&pack.frame.exchange);
    let targets = &state.targets;
    let source = io.scene_colour.texture();
    if io.viewport != [0, 0, source.width(), source.height()] {
        if let Some((_, pipeline)) = pack
            .host
            .preserve
            .iter()
            .find(|(f, _)| *f == io.destination_format)
        {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader pack preserve viewport exterior"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(io.scene_colour),
                }],
            });
            let mut render = pass(
                encoder,
                "shader pack preserve viewport exterior",
                &[load_attachment(io.destination)],
                None,
            );
            render.set_pipeline(pipeline);
            render.set_bind_group(0, &group, &[]);
            render.draw(0..3, 0..1);
        }
    }
    let present = pack
        .host
        .present
        .iter()
        .find(|(f, _)| *f == io.destination_format);
    let group = state.bindings.present[targets.parity]
        .iter()
        .find(|(f, _)| *f == io.destination_format);
    if let (Some((_, pipeline)), Some((_, group))) = (present, group) {
        let mut render = pass(
            encoder,
            "shader pack present",
            &[load_attachment(io.destination)],
            None,
        );
        let [x, y, w, h] = io.viewport.map(|v| v as f32);
        render.set_viewport(x, y, w, h, 0.0, 1.0);
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// Writes the planes and limits the host's depth conversions and the Vanilla overlay use.
fn write_depth_parameters(queue: &Queue, buffer: &wgpu::Buffer, io: &FrameIo<'_>) {
    let far = io.render_distance.max(16.0) * FAR_PLANE_SCALE;
    // Vanilla content beyond `far` (its far-terrain impostors) has no pack geometry behind it
    // and never comes through; inside, only what lies within the fully expanded radius does.
    let overlay_radius = io
        .geometry
        .as_ref()
        .map_or(f32::MAX, |g| g.overlay_radius)
        .min(io.render_distance.max(16.0));
    let [x, y, w, h] = io.viewport.map(|v| v as f32);
    let parameters = [
        io.scene_near,
        GL_NEAR,
        far,
        overlay_radius,
        io.inverse_focal[0],
        io.inverse_focal[1],
        0.0,
        0.0,
        x,
        y,
        w,
        h,
    ];
    let bytes: Vec<u8> = parameters.iter().flat_map(|v| v.to_le_bytes()).collect();
    queue.write_buffer(buffer, 0, &bytes);
}

/// Which of the pack's pass lists to run.
#[derive(Clone, Copy)]
enum Steps {
    Shadowcomp,
    Prepare,
    Deferred,
    Composite,
}

/// Runs one pass list; compute passes size their dispatch from `size`.
fn run_steps(encoder: &mut CommandEncoder, state: &mut ViewState, steps: Steps, size: [u32; 2]) {
    let pack = Arc::clone(&state.pack);
    let start = pack.frame.composite_start;
    let (steps, groups) = match steps {
        Steps::Shadowcomp => (&pack.shadowcomp[..], &state.bindings.shadowcomp[..]),
        Steps::Prepare => (&pack.frame.prepare[..], &state.bindings.prepare[..]),
        Steps::Deferred => (&pack.frame.post[..start], &state.bindings.post[..start]),
        Steps::Composite => (&pack.frame.post[start..], &state.bindings.post[start..]),
    };
    let parity = state.targets.parity;
    for (step, groups) in steps.iter().zip(groups) {
        run_step(
            encoder,
            &pack,
            &state.targets,
            &state.bindings,
            step,
            &groups[parity],
            &mut state.skipped,
            size,
        );
    }
}

/// Copies the scene colour into `colortex0` and the scene depth into `depthtex*` and the
/// geometry depth, which `depthtex0` reads.
fn copy_scene(
    device: &Device,
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    targets: &ViewTargets,
    depth_parameters: &wgpu::Buffer,
    io: &FrameIo<'_>,
) {
    let host = &pack.host;
    if let Some(buffer) = targets.buffers[0].as_ref() {
        let view = &buffer.attachment[buffer.side(pack.frame.scene_alt & 1 != 0)];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader pack scene colour"),
            layout: &host.scene_colour.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(io.scene_colour),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: depth_parameters.as_entire_binding(),
                },
            ],
        });
        let mut render = pass(
            encoder,
            "shader pack scene colour",
            &[load_attachment(view)],
            None,
        );
        render.set_pipeline(&host.scene_colour);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..3, 0..1);
    }
    let depth_group = |pipeline: &wgpu::RenderPipeline| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader pack scene depth"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(io.scene_depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: depth_parameters.as_entire_binding(),
                },
            ],
        })
    };
    if let Some(pipeline) = &host.scene_depth {
        let group = depth_group(pipeline);
        let attachments: Vec<_> = targets
            .depth
            .iter()
            .map(|d| d.as_ref().and_then(|d| load_attachment(&d.view)))
            .collect();
        let mut render = pass(encoder, "shader pack scene depth", &attachments, None);
        render.set_pipeline(pipeline);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..3, 0..1);
    }
    if let Some(pipeline) = &host.scene_depth_attachment {
        let group = depth_group(pipeline);
        let depths = targets.depth_compare.iter().flatten();
        for target in depths.chain(&targets.geometry_depth) {
            let depth = wgpu::RenderPassDepthStencilAttachment {
                view: &target.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            };
            let mut render = pass(
                encoder,
                "shader pack scene comparison depth",
                &[],
                Some(depth),
            );
            render.set_pipeline(pipeline);
            render.set_bind_group(0, &group, &[]);
            render.draw(0..3, 0..1);
        }
    }
}

/// Regenerates the mip chain of a buffer's current side.
fn generate_mipmaps(
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    targets: &ViewTargets,
    bindings: &ViewBindings,
    index: u32,
    read_alt: u32,
) {
    let Some(buffer) = targets.buffers[index as usize].as_ref() else {
        return;
    };
    let side = buffer.side(read_alt & (1 << index) != 0);
    let Some(format) = pack.buffer_formats[index as usize] else {
        return;
    };
    let Some((_, pipeline)) = pack.host.downsample.iter().find(|(f, _)| *f == format) else {
        return;
    };
    let Some((_, _, groups)) = bindings.downsample[targets.parity]
        .iter()
        .find(|(b, s, _)| *b == index && *s == side)
    else {
        return;
    };
    for (level, group) in groups.iter().enumerate() {
        let target = &buffer.levels[side][level + 1];
        let mut render = pass(
            encoder,
            "shader pack mipmap",
            &[load_attachment(target)],
            None,
        );
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// Runs one program; render passes whose attachments disagree in size are skipped (logged
/// once per program), as OpenGL cannot draw into them either. Render-relative compute
/// dispatches scale `size` (the screen, or the shadow map for `shadowcomp`).
#[allow(clippy::too_many_arguments, reason = "one pass's frame state")]
fn run_step(
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    targets: &ViewTargets,
    bindings: &ViewBindings,
    step: &PassStep,
    groups: &Groups,
    skipped: &mut BTreeSet<String>,
    size: [u32; 2],
) {
    for &index in &step.mipmaps {
        generate_mipmaps(encoder, pack, targets, bindings, index, step.read_alt);
    }
    let program = &pack.programs[step.program];
    targets.take_snapshots(encoder, program, step.read_alt);
    match &program.pipeline {
        ProgramPipeline::Geometry | ProgramPipeline::Inert => {}
        ProgramPipeline::Compute(pipeline) => {
            let Some(dispatch) = &program.reflection.compute else {
                return;
            };
            let groups_count = dispatch.work_groups.unwrap_or_else(|| {
                let scale = dispatch.work_groups_render.unwrap_or([1.0, 1.0]);
                std::array::from_fn(|i| {
                    if i == 2 {
                        1
                    } else {
                        ((size[i] as f32 * scale[i]).ceil() as u32)
                            .div_ceil(dispatch.workgroup_size[i].max(1))
                            .max(1)
                    }
                })
            });
            let mut compute = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&program.name),
                    timestamp_writes: None,
                })
                .forget_lifetime();
            compute.set_pipeline(pipeline);
            for (i, group) in groups.iter().enumerate() {
                if let Some(group) = group {
                    compute.set_bind_group(i as u32, group, &[]);
                }
            }
            compute.dispatch_workgroups(groups_count[0], groups_count[1], groups_count[2]);
        }
        ProgramPipeline::Render(pipeline) | ProgramPipeline::StoreOnly(pipeline) => {
            debug_assert!(!is_compute(pack, step));
            let mut size = None;
            let mut consistent = true;
            let attachments: Vec<_> = if matches!(program.pipeline, ProgramPipeline::StoreOnly(_)) {
                size = Some(targets.screen_size);
                vec![load_attachment(&targets.screen.view)]
            } else {
                program
                    .color_targets
                    .iter()
                    .enumerate()
                    .map(|(location, present)| {
                        if !present {
                            return None;
                        }
                        let (view, extent) = if program.group == ProgramGroup::Final {
                            if location != 0 {
                                return None;
                            }
                            (&targets.screen.view, targets.screen_size)
                        } else {
                            let (index, alt) = step.writes.get(location).copied().flatten()?;
                            let buffer = targets.buffers[index as usize].as_ref()?;
                            (&buffer.attachment[buffer.side(alt)], buffer.size)
                        };
                        consistent &= *size.get_or_insert(extent) == extent;
                        load_attachment(view)
                    })
                    .collect()
            };
            let Some(extent) = size.filter(|_| consistent) else {
                if skipped.insert(program.name.clone()) {
                    bevy::log::warn!(
                        target: "cinnabar::shaderpack",
                        pack = %pack.name, program = %program.name,
                        "skipped: no drawable outputs or outputs of different sizes"
                    );
                }
                return;
            };
            let mut render = pass(encoder, &program.name, &attachments, None);
            if let Some(scale) = &program.scale {
                // Rows are stored bottom-up, so the GL viewport origin maps unchanged.
                let [w, h] = extent.map(|v| v as f32);
                let x = (scale.offset[0] * w).clamp(0.0, w - 1.0);
                let y = (scale.offset[1] * h).clamp(0.0, h - 1.0);
                render.set_viewport(
                    x,
                    y,
                    (scale.scale * w).clamp(1.0, w - x),
                    (scale.scale * h).clamp(1.0, h - y),
                    0.0,
                    1.0,
                );
            }
            render.set_pipeline(pipeline);
            for (i, group) in groups.iter().enumerate() {
                if let Some(group) = group {
                    render.set_bind_group(i as u32, group, &[]);
                }
            }
            render.set_vertex_buffer(0, pack.quad.slice(..));
            render.draw(0..4, 0..1);
        }
    }
}
