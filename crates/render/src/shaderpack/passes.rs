//! Records a frame of a pack: clears, `prepare*`, the Vanilla scene copy into `colortex0`
//! and `depthtex*`, `deferred*`, `composite*` (with compute variants), `final` into the
//! screen, the copy-back (or exchange of textures) of buffers that end flipped, and the
//! screen's presentation into the view.

use super::loader::{PackGpu, ProgramPipeline};
use super::plan::PassStep;
use super::targets::{Groups, ViewBindings, ViewTargets, is_compute};
use super::uniforms::{FAR_PLANE_SCALE, FrameUniforms, GL_NEAR, HostValue, pack_block};
use crate::gpu_timing::{GpuTimestamps, within_stage};
use render_scene::RuntimeStage;
use shaderpack::{CustomUniforms, ProgramGroup};
use std::collections::BTreeSet;
use std::sync::Arc;
use wgpu::{CommandEncoder, Device, Queue, TextureFormat, TextureView};

/// Conversion planes and viewport rectangle uploaded to the host scene-copy shaders.
pub(super) type SceneDepthParameters = [f32; 8];

/// One view's resources and per-frame state for a loaded pack.
pub(crate) struct ViewState {
    pub pack: Arc<PackGpu>,
    pub targets: ViewTargets,
    pub bindings: ViewBindings,
    /// View-local scene conversion planes and source rectangle.
    depth_parameters: wgpu::Buffer,
    scene_bindings: super::scene_bindings::SceneBindingCache,
    pub history: super::uniforms::UniformHistory,
    custom: CustomUniforms,
    scratch: Vec<u8>,
    unknown: BTreeSet<String>,
    unknown_logged: bool,
    /// The developer uniform dump, when [`super::uniform_audit::DUMP_ENV`] names a file.
    audit: Option<super::uniform_audit::UniformAudit>,
    /// Programs already reported as skipped.
    skipped: BTreeSet<String>,
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
                size: std::mem::size_of::<SceneDepthParameters>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            pack,
            scene_bindings: Default::default(),
            targets,
            bindings,
            history: Default::default(),
            custom,
            scratch: vec![],
            unknown: BTreeSet::new(),
            unknown_logged: false,
            audit: super::uniform_audit::UniformAudit::from_env(),
            skipped: BTreeSet::new(),
            in_world: false,
        })
    }

    /// Whether the state still matches `pack` and the screen size.
    pub(crate) fn matches(&self, pack: &Arc<PackGpu>, screen: [u32; 2]) -> bool {
        Arc::ptr_eq(&self.pack, pack) && self.targets.screen_size == screen
    }

    /// Whether both the presentation pipeline and its view bindings support this format.
    pub(crate) fn supports_presentation(&self, format: TextureFormat) -> bool {
        self.pack.host.present.iter().any(|(f, _)| *f == format)
            && self.bindings.present[self.targets.parity]
                .iter()
                .any(|(f, _)| *f == format)
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

    /// Evaluates custom uniforms and writes every program's uniform block.
    pub(crate) fn write_uniforms(&mut self, queue: &Queue, frame: &FrameUniforms, frame_time: f32) {
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
    /// `far`: the render distance in blocks.
    pub render_distance: f32,
    /// `fogColor`, which `colortex0` clears to by default.
    pub fog_color: [f32; 3],
    /// Where the presented frame goes, and its format.
    pub destination: &'a TextureView,
    pub destination_format: TextureFormat,
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
    pass_with_timestamps(encoder, label, attachments, depth, None)
}

/// Opens an owned render pass with optional queries on the existing pass boundaries.
fn pass_with_timestamps<'a>(
    encoder: &'a mut CommandEncoder,
    label: &str,
    attachments: &[Option<wgpu::RenderPassColorAttachment<'_>>],
    depth: Option<wgpu::RenderPassDepthStencilAttachment<'_>>,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'a> {
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: attachments,
            depth_stencil_attachment: depth,
            timestamp_writes: timestamps,
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
    state.scene_bindings.prepare(
        device,
        &pack.host,
        &state.depth_parameters,
        io.scene_colour,
        io.scene_depth,
        io.destination_format,
    );
    write_depth_parameters(queue, &state.depth_parameters, io);
    state.targets.clear_images(encoder, &pack.custom_images);
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
    let timestamps = io.timestamps;
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackOpaque,
        encoder,
        |encoder| {
            run_steps(
                encoder,
                state,
                Steps::Prepare,
                state.targets.screen_size,
                timestamps,
            );
            copy_scene(
                encoder,
                &pack,
                &state.targets,
                state.scene_bindings.current(),
                io,
            );
        },
    );
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackDeferred,
        encoder,
        |encoder| {
            run_steps(
                encoder,
                state,
                Steps::Deferred,
                state.targets.screen_size,
                timestamps,
            )
        },
    );
    within_stage(
        timestamps,
        RuntimeStage::GpuShaderpackComposite,
        encoder,
        |encoder| record_composite(encoder, state, io),
    );
}

/// `composite*` and `final`, the copy-back of buffers that end flipped (or the exchange of
/// their textures), and the screen's presentation into the view.
fn record_composite(encoder: &mut CommandEncoder, state: &mut ViewState, io: &FrameIo<'_>) {
    let pack = Arc::clone(&state.pack);
    run_steps(
        encoder,
        state,
        Steps::Composite,
        state.targets.screen_size,
        io.timestamps,
    );
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
    if io.viewport != [0, 0, source.width(), source.height()]
        && let Some((_, pipeline)) = pack
            .host
            .preserve
            .iter()
            .find(|(f, _)| *f == io.destination_format)
    {
        let group = state
            .scene_bindings
            .current()
            .preserve
            .as_ref()
            .expect("the preserve pipeline has bindings");
        let mut render = pass_with_timestamps(
            encoder,
            "shader pack preserve viewport exterior",
            &[load_attachment(io.destination)],
            None,
            io.timestamps.and_then(|timestamps| {
                timestamps.shaderpack_render_pass(RuntimeStage::GpuShaderpackComposite)
            }),
        );
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
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
        let mut render = pass_with_timestamps(
            encoder,
            "shader pack present",
            &[load_attachment(io.destination)],
            None,
            io.timestamps.and_then(|timestamps| {
                timestamps.shaderpack_render_pass(RuntimeStage::GpuShaderpackComposite)
            }),
        );
        let [x, y, w, h] = io.viewport.map(|v| v as f32);
        render.set_viewport(x, y, w, h, 0.0, 1.0);
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// Writes the planes the host's depth conversions use.
fn write_depth_parameters(queue: &Queue, buffer: &wgpu::Buffer, io: &FrameIo<'_>) {
    let far = io.render_distance.max(16.0) * FAR_PLANE_SCALE;
    let [x, y, w, h] = io.viewport.map(|v| v as f32);
    let parameters: SceneDepthParameters = [io.scene_near, GL_NEAR, far, 0.0, x, y, w, h];
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(&parameters));
}

/// Which of the pack's pass lists to run.
#[derive(Clone, Copy)]
enum Steps {
    Prepare,
    Deferred,
    Composite,
}

/// Runs one pass list; compute passes size their dispatch from `size`.
fn run_steps(
    encoder: &mut CommandEncoder,
    state: &mut ViewState,
    steps: Steps,
    size: [u32; 2],
    timestamps: Option<&GpuTimestamps>,
) {
    let pack = Arc::clone(&state.pack);
    let start = pack.frame.composite_start;
    let (steps, groups, stage) = match steps {
        Steps::Prepare => (
            &pack.frame.prepare[..],
            &state.bindings.prepare[..],
            RuntimeStage::GpuShaderpackOpaque,
        ),
        Steps::Deferred => (
            &pack.frame.post[..start],
            &state.bindings.post[..start],
            RuntimeStage::GpuShaderpackDeferred,
        ),
        Steps::Composite => (
            &pack.frame.post[start..],
            &state.bindings.post[start..],
            RuntimeStage::GpuShaderpackComposite,
        ),
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
            timestamps,
            stage,
        );
    }
}

/// Copies the scene colour into `colortex0` and the scene depth into `depthtex*`.
fn copy_scene(
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    targets: &ViewTargets,
    bindings: &super::scene_bindings::SceneBindings,
    io: &FrameIo<'_>,
) {
    let host = &pack.host;
    if let Some(buffer) = targets.buffers[0].as_ref() {
        let view = &buffer.attachment[buffer.side(pack.frame.scene_alt & 1 != 0)];
        let mut render = pass_with_timestamps(
            encoder,
            "shader pack scene colour",
            &[load_attachment(view)],
            None,
            io.timestamps.and_then(|timestamps| {
                timestamps.shaderpack_render_pass(RuntimeStage::GpuShaderpackOpaque)
            }),
        );
        render.set_pipeline(&host.scene_colour);
        render.set_bind_group(0, &bindings.colour, &[]);
        render.draw(0..3, 0..1);
    }
    if let Some(pipeline) = &host.scene_depth {
        let group = bindings
            .depth
            .as_ref()
            .expect("the depth pipeline has bindings");
        let attachments = targets
            .depth
            .each_ref()
            .map(|d| d.as_ref().and_then(|d| load_attachment(&d.view)));
        let mut render = pass_with_timestamps(
            encoder,
            "shader pack scene depth",
            &attachments,
            None,
            io.timestamps.and_then(|timestamps| {
                timestamps.shaderpack_render_pass(RuntimeStage::GpuShaderpackOpaque)
            }),
        );
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
    }
    if let Some(pipeline) = &host.scene_depth_attachment {
        let group = bindings
            .comparison_depth
            .as_ref()
            .expect("the comparison depth pipeline has bindings");
        for target in targets.depth_compare.iter().flatten() {
            let depth = wgpu::RenderPassDepthStencilAttachment {
                view: &target.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            };
            let mut render = pass_with_timestamps(
                encoder,
                "shader pack scene comparison depth",
                &[],
                Some(depth),
                io.timestamps.and_then(|timestamps| {
                    timestamps.shaderpack_render_pass(RuntimeStage::GpuShaderpackOpaque)
                }),
            );
            render.set_pipeline(pipeline);
            render.set_bind_group(0, group, &[]);
            render.draw(0..3, 0..1);
        }
    }
}

/// Regenerates the mip chain of a buffer's current side.
#[allow(
    clippy::too_many_arguments,
    reason = "one buffer mip chain and its timing"
)]
fn generate_mipmaps(
    encoder: &mut CommandEncoder,
    pack: &PackGpu,
    targets: &ViewTargets,
    bindings: &ViewBindings,
    index: u32,
    read_alt: u32,
    timestamps: Option<&GpuTimestamps>,
    stage: RuntimeStage,
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
        let mut render = pass_with_timestamps(
            encoder,
            "shader pack mipmap",
            &[load_attachment(target)],
            None,
            timestamps.and_then(|timestamps| timestamps.shaderpack_render_pass(stage)),
        );
        render.set_pipeline(pipeline);
        render.set_bind_group(0, group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// Runs one program; render passes whose attachments disagree in size are skipped (logged
/// once per program), as OpenGL cannot draw into them either. Render-relative compute
/// dispatches scale `size`, the screen.
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
    timestamps: Option<&GpuTimestamps>,
    stage: RuntimeStage,
) {
    for &index in &step.mipmaps {
        generate_mipmaps(
            encoder,
            pack,
            targets,
            bindings,
            index,
            step.read_alt,
            timestamps,
            stage,
        );
    }
    let program = &pack.programs[step.program];
    targets.take_snapshots(encoder, program, step.read_alt);
    match &program.pipeline {
        ProgramPipeline::Inert => {}
        ProgramPipeline::Compute(pipeline) => {
            let Some(dispatch) = &program.reflection.compute else {
                return;
            };
            let groups_count = super::dispatch::groups(dispatch, size);
            let mut compute = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&program.name),
                    timestamp_writes: timestamps
                        .and_then(|timestamps| timestamps.shaderpack_compute_pass(stage)),
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
            let mut render = pass_with_timestamps(
                encoder,
                &program.name,
                &attachments,
                None,
                timestamps.and_then(|timestamps| timestamps.shaderpack_render_pass(stage)),
            );
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
