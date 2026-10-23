//! Turns a shader pack into GPU-ready programs on a worker thread.
//!
//! The worker opens the pack, translates the selected dimension, logs every program's
//! diagnostics, plans buffers and passes, and builds pipelines and pack-wide textures with
//! the shared device. Translation takes tens of milliseconds; driver pipeline compilation
//! dominates the load, so the cache keeps the backend pipeline cache (Vulkan) in a private
//! directory, keyed by the pack's content hash, the translation settings and the adapter.

mod planning;

use planning::{pack_settings, planned, storage_sizes};
#[path = "load_diagnostics.rs"]
mod diagnostics;
use diagnostics::{failure_line, log_program};

use super::draws::{GeometryDraw, GeometryStage, geometry_draws, select_draws};
use super::effects::EffectSettings;
use super::host_pipelines::{HostPipelines, host_pipelines};
use super::pipeline_cache::PipelineCacheFile;
use super::pipelines::{
    GpuContext, Kind, MASKED_TARGET, bind_group_layouts, class_sampler, color_targets, format_kind,
    layout_entries, quad_attributes, quad_layout, shader_modules, storage_format, stores,
    texture_format,
};
use super::plan::{
    BufferPlan, FramePlan, MAX_COLOR_BUFFERS, PassStep, PlannedProgram, color_buffer_of,
    exchange_swap_back, plan_buffers, plan_frame, skip_cleared_swap_back,
};
use super::resources::bindings::resolve_bindings;
use super::resources::vanilla::VanillaTextures;
use super::resources::{
    ATLAS_FORMAT, ConstantBuilder, ConstantTextures, ImageSource, TextureSource, image_formats,
};
use super::shadow_maps::{ShadowMaps, shadow_maps};
use super::uniforms::PackSettings;
use render_scene::shaderpack::LoadProgress;
use sha2::{Digest, Sha256};
#[cfg(test)]
use shaderpack::OptionValues;
use shaderpack::{
    Environment, ProgramGroup, ProgramKind, ProgramReflection, ProgramTranslation, SamplerBinding,
    SamplerClass, ScaleRule, ShaderPack, StageKind, TranslateConfig,
};
use std::path::Path;
use std::sync::Arc;
use wgpu::{TextureFormat, TextureUsages};

/// Bumped when translated output or the cache layout changes, invalidating cached data.
const CACHE_VERSION: u32 = 2;

/// The GPU pipeline of one program.
pub(crate) enum ProgramPipeline {
    Render(wgpu::RenderPipeline),
    /// A full-screen program that writes no colour buffer the frame has but stores to images
    /// or storage buffers. WebGPU needs an attachment where OpenGL does not, so it draws into
    /// the screen with every channel masked off; only its stores take effect.
    StoreOnly(wgpu::RenderPipeline),
    /// A full-screen program with no colour buffer to write and nothing to store, such as
    /// a pack's switched-off pass (`void main() {}`): Iris runs it to no effect, so it
    /// has no pipeline.
    Inert,
    Compute(wgpu::ComputePipeline),
    /// A geometry program: its pipelines are per draw, in [`PackGpu::draws`].
    Geometry,
}

/// One runnable program with its pipeline and resolved bindings.
pub(crate) struct GpuProgram {
    pub name: String,
    pub group: ProgramGroup,
    pub reflection: ProgramReflection,
    pub pipeline: ProgramPipeline,
    pub layouts: [Option<wgpu::BindGroupLayout>; 4],
    /// Parallel to `reflection.textures`.
    pub textures: Vec<TextureSource>,
    /// Parallel to `reflection.storage_images`.
    pub images: Vec<ImageSource>,
    pub scale: Option<ScaleRule>,
    /// Per fragment output location: the pipeline has a colour target there.
    pub color_targets: Vec<bool>,
}

impl GpuProgram {
    /// Bit per colour buffer the program both samples (`colortexN`) and writes as a storage
    /// image (`colorimgN`). Both bind the same side, which OpenGL allows and wgpu forbids
    /// within one pass, so the host samples a copy taken just before the pass.
    pub fn sampled_while_written(&self) -> u32 {
        let bits = |indices: &mut dyn Iterator<Item = u32>| indices.fold(0, |b, i| b | 1 << i);
        let sampled = bits(&mut self.textures.iter().filter_map(|s| match *s {
            TextureSource::Buffer { index, .. } => Some(index),
            _ => None,
        }));
        let written = bits(&mut self.images.iter().filter_map(|s| match *s {
            ImageSource::Buffer(index) => Some(index),
            _ => None,
        }));
        sampled & written
    }

    /// Whether the program both samples custom image `index` (its sampler name) and writes
    /// it as a storage image, as ping-pong compute passes do (Solas's floodfill reads one
    /// image and writes the other, binding both ways each frame). OpenGL allows it; wgpu
    /// forbids it within one dispatch, so the host samples a copy taken before the pass.
    pub fn image_sampled_while_written(&self, index: usize) -> bool {
        self.textures
            .iter()
            .any(|s| matches!(*s, TextureSource::Image { index: i, .. } if i == index))
            && self
                .images
                .iter()
                .any(|s| matches!(*s, ImageSource::Image(i) if i == index))
    }
}

/// A loaded pack, immutable and shared by every view.
pub(crate) struct PackGpu {
    pub name: String,
    pub programs: Vec<GpuProgram>,
    pub frame: FramePlan,
    pub buffers: Vec<BufferPlan>,
    /// Texture format of each planned buffer.
    pub buffer_formats: [Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    /// Buffers some program binds as `colorimgN`.
    pub storage_buffers: u32,
    /// Buffers some program uses with image atomics (`STORAGE_ATOMIC`).
    pub atomic_buffers: u32,
    /// Format of the `depthtex*` copies sampled as floats.
    pub depth_format: TextureFormat,
    /// `depthtex0..2` some program samples with the comparison sampler.
    pub depth_compare: [bool; 3],
    /// `depthtex0..2` some program samples as floats; the others are never copied.
    pub depth_sampled: [bool; 3],
    pub constants: ConstantTextures,
    pub custom_images: Vec<shaderpack::CustomImage>,
    /// Texture format of each custom image.
    pub image_formats: Vec<TextureFormat>,
    /// Whether some program uses each custom image with image atomics (`STORAGE_ATOMIC`).
    pub atomic_images: Vec<bool>,
    /// Storage-buffer bindings with their byte sizes.
    pub shader_storage: Vec<(u32, u64)>,
    pub settings: PackSettings,
    pub custom_uniforms: shaderpack::CustomUniforms,
    /// One filtering sampler per [`SamplerClass`], in [`SamplerClass::ALL`] order.
    pub filtering: [wgpu::Sampler; SamplerClass::ALL.len()],
    pub comparison: wgpu::Sampler,
    pub zero_uniforms: wgpu::Buffer,
    pub quad: wgpu::Buffer,
    pub host: HostPipelines,
    /// Geometry draws in recording order; empty when the pack has no terrain program, in
    /// which case the Vanilla scene stands in for the gbuffers.
    pub draws: Vec<GeometryDraw>,
    /// The expanded vertex the pack's terrain draws read.
    pub terrain_layout: super::geometry::TerrainLayout,
    /// `shadowcomp*` compute passes after the shadow pass.
    pub shadowcomp: Vec<PassStep>,
    /// Present when some shadow draw resolved.
    pub shadow: Option<ShadowMaps>,
    /// `shadowTerrain` and `shadowTranslucent`: whether the shadow pass draws opaque and
    /// translucent terrain (both by default).
    pub shadow_terrain: bool,
    pub shadow_translucent: bool,
    /// `mc_Entity` id per Bedrock block state (by sequential id) from `block.properties`.
    pub block_ids: Option<Arc<Vec<i32>>>,
    /// `entityId` and item ids per Bedrock identifier, including full-screen-only packs.
    pub actor_ids: Arc<super::ids::ActorIdMaps>,
    /// `separateAo` and `oldLighting` from `shaders.properties`.
    pub separate_ao: bool,
    pub old_lighting: bool,
    /// Pack-wide placeholder bound for the atlas until Cinnabar's atlas is built.
    pub atlas_placeholder: usize,
    /// Pack-wide `lightmap` texture some geometry program samples; rewritten each frame the
    /// world lightmap changes.
    pub lightmap: Option<usize>,
    /// The pack's effect directives (clouds, sun, moon, stars, weather, particle ordering).
    pub effects: EffectSettings,
}

impl PackGpu {
    /// The sampler bound for a reflected sampler binding.
    pub fn sampler(&self, binding: &SamplerBinding) -> &wgpu::Sampler {
        match binding.class() {
            Some(class) => &self.filtering[class as usize],
            None => &self.comparison,
        }
    }
}

/// Full-screen groups whose programs must all translate for the pack to load. Geometry
/// programs (`gbuffers_*`, `shadow*`) fall back along Iris's chains instead, and
/// `shadowcomp*` compute passes are optional.
fn runs(group: ProgramGroup) -> bool {
    matches!(
        group,
        ProgramGroup::Prepare
            | ProgramGroup::Deferred
            | ProgramGroup::Composite
            | ProgramGroup::Final
    )
}

/// The pack's content hash with the translation settings: the cache key.
pub(crate) fn content_key(pack: &ShaderPack, config: &TranslateConfig) -> String {
    let mut hash = Sha256::new();
    hash.update(CACHE_VERSION.to_le_bytes());
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    for (path, text) in pack.source().files() {
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    }
    hash.update(format!("{config:?}").as_bytes());
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The translation settings for this host.
#[cfg(test)]
pub(crate) fn translate_config(pack: &ShaderPack, dimension: &str) -> TranslateConfig {
    let environment = host_environment();
    TranslateConfig {
        folder: pack.dimension_folder(dimension, &environment),
        environment,
        ..TranslateConfig::default()
    }
}

/// Supplies the current host's macros to both local fixtures and the isolated compiler.
fn host_environment() -> Environment {
    Environment {
        os: if cfg!(target_os = "windows") {
            shaderpack::HostOs::Windows
        } else if cfg!(target_os = "macos") {
            shaderpack::HostOs::MacOs
        } else if cfg!(target_os = "linux") {
            shaderpack::HostOs::Linux
        } else {
            shaderpack::HostOs::Other
        },
        ..Environment::default()
    }
}

/// Stops retired workers before opening files or submitting another GPU construction batch.
fn check_cancelled(progress: &LoadProgress) -> Result<(), String> {
    if progress.is_cancelled() {
        Err("shader pack load cancelled".to_owned())
    } else {
        Ok(())
    }
}

/// Opens, translates and builds `path` for `dimension` (a Java dimension id) with the values
/// saved in `options` (the pack's Iris option file, if any); `vanilla` answers the pack's
/// `minecraft:` texture references. Saved values are normalized in memory; the option editor
/// owns file writes so a load worker cannot overwrite a newer edit. Translation reports into
/// `progress`.
#[expect(
    clippy::too_many_arguments,
    reason = "the load's paths, GPU and cancellation inputs"
)]
pub(crate) fn load(
    path: &Path,
    options: Option<&Path>,
    dimension: &str,
    cache_dir: Option<&Path>,
    gpu: &GpuContext,
    vanilla: &VanillaTextures,
    progress: &LoadProgress,
    compiler: Option<&Path>,
) -> Result<PackGpu, String> {
    super::kernels::validate_device(&gpu.device.limits())?;
    check_cancelled(progress)?;
    let started = std::time::Instant::now();
    let request = shaderpack::worker::Request {
        pack: path.to_owned(),
        options: options.map(Path::to_owned),
        dimension: dimension.to_owned(),
        environment: host_environment(),
        purpose: shaderpack::worker::Purpose::Render,
    };
    let executable = compiler.ok_or("the isolated shader compiler is unavailable")?;
    let mut worker = shaderpack::worker::Running::start(executable, &request)?;
    let compiled = loop {
        if let Some(result) = worker.poll(started.elapsed(), progress.is_cancelled()) {
            break result?.into_translation()?;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(10));
    };
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    build_translation(compiled, &name, cache_dir, gpu, vanilla, progress, started)
}

/// The enabled programs this host runs, in frame order, with validated destinations.
/// Translation failures name each program and its first diagnostic.
pub(crate) fn runnable_programs(
    translation: &shaderpack::PackTranslation,
) -> Result<(Vec<&ProgramTranslation>, Vec<PlannedProgram>), String> {
    let mut failed = vec![];
    let mut runnable = vec![];
    let mut plans = vec![];
    for program in &translation.programs {
        if !program.enabled || !runs(program.program.group) {
            continue;
        }
        match &program.reflection {
            Some(reflection) => {
                plans.push(planned(program, reflection, &translation.properties)?);
                runnable.push(program);
            }
            None => failed.push(failure_line(program)),
        }
    }
    if failed.is_empty() {
        Ok((runnable, plans))
    } else {
        Err(failed.join("\n"))
    }
}

/// Translates and builds an opened pack with option `values`; translation reports into
/// `progress`.
#[expect(
    clippy::too_many_arguments,
    reason = "the pack, its option values and the load's host inputs"
)]
#[cfg(test)]
pub(crate) fn build(
    pack: &ShaderPack,
    name: &str,
    dimension: &str,
    values: &OptionValues,
    cache_dir: Option<&Path>,
    gpu: &GpuContext,
    vanilla: &VanillaTextures,
    progress: &LoadProgress,
) -> Result<PackGpu, String> {
    let config = TranslateConfig {
        options: values.clone(),
        ..translate_config(pack, dimension)
    };
    let started = std::time::Instant::now();
    check_cancelled(progress)?;
    let translation = pack
        .translate_cancellable(
            &config,
            |translated, programs| progress.record(translated, programs),
            || progress.is_cancelled(),
        )
        .ok_or_else(|| "shader pack load cancelled".to_owned())?;
    build_translation(
        shaderpack::worker::prepare_translation(pack.clone(), config, translation),
        name,
        cache_dir,
        gpu,
        vanilla,
        progress,
        started,
    )
}

/// Builds GPU resources from a completed CPU translation; no pack metadata is compiled here.
fn build_translation(
    compiled: shaderpack::worker::Output,
    name: &str,
    cache_dir: Option<&Path>,
    gpu: &GpuContext,
    vanilla: &VanillaTextures,
    progress: &LoadProgress,
    started: std::time::Instant,
) -> Result<PackGpu, String> {
    super::kernels::validate_device(&gpu.device.limits())?;
    let shaderpack::worker::Output {
        pack,
        config,
        translation,
        custom_uniforms: (custom_uniforms, errors),
        ids,
        block_ids,
        options_warning,
    } = compiled;
    let pack = &pack;
    if let Some(error) = options_warning {
        bevy::log::warn!(target: "cinnabar::shaderpack", pack = name,
            "shader pack options unreadable, using defaults: {error}");
    }
    for error in errors {
        bevy::log::debug!(target: "cinnabar::shaderpack", pack = name, line = error.line,
            name = %error.name, "custom uniform dropped: {}", error.message);
    }
    check_cancelled(progress)?;
    for diagnostic in &translation.properties.diagnostics {
        bevy::log::debug!(
            target: "cinnabar::shaderpack",
            pack = name, line = diagnostic.line,
            "shaders.properties: {}", diagnostic.message
        );
    }
    for program in &translation.programs {
        log_program(name, program);
    }
    let (runnable, plans) = runnable_programs(&translation)?;
    for program in &runnable {
        if let Some(dispatch) = program.reflection.as_ref().and_then(|r| r.compute.as_ref())
            && dispatch.work_groups.is_some()
        {
            super::dispatch::validate(dispatch, [1, 1], &gpu.device.limits())
                .map_err(|error| format!("{}: {error}", program.program.name))?;
        }
    }
    let (block_ids, actor_ids) = super::ids::id_tables(ids, block_ids, name);
    let effects = EffectSettings::read(
        &translation.properties,
        runnable
            .iter()
            .any(|p| p.program.group == ProgramGroup::Deferred),
        assets::facts::entity::NativeEntityClass::LightningBolt
            .identifier()
            .and_then(|lightning| actor_ids.entity_id(lightning)),
    );
    let (geometry, selected_draws) =
        select_draws(&translation.programs, |kind| kind.drawn_by(&effects));
    let shadowcomp: Vec<&ProgramTranslation> = translation
        .programs
        .iter()
        .filter(|p| p.enabled && p.program.group == ProgramGroup::ShadowComposite)
        .filter(|p| {
            let compute = p.program.kind == ProgramKind::Compute && p.reflection.is_some();
            if !compute {
                bevy::log::warn!(
                    target: "cinnabar::shaderpack",
                    pack = name, program = %p.program.name,
                    "skipped: only translated shadowcomp compute programs run"
                );
            }
            compute
        })
        .collect();
    bevy::log::info!(
        target: "cinnabar::shaderpack",
        pack = name, folder = %config.folder,
        programs = translation.programs.len(), runnable = runnable.len(),
        geometry = ?selected_draws
            .iter()
            .map(|(kind, i)| format!("{kind:?}={}", geometry[*i].program.name))
            .collect::<Vec<_>>(),
        "translated in {:.0?}", started.elapsed()
    );
    // Full-screen programs first (frame-plan indices), then geometry, then shadowcomp.
    let all: Vec<&ProgramTranslation> = runnable
        .iter()
        .chain(&geometry)
        .chain(&shadowcomp)
        .copied()
        .collect();
    let reflections: Vec<&ProgramReflection> =
        all.iter().filter_map(|p| p.reflection.as_ref()).collect();
    let geometry_plans: Vec<PlannedProgram> = geometry
        .iter()
        .filter(|p| p.program.group == ProgramGroup::Gbuffers)
        .filter_map(|p| {
            p.reflection
                .as_ref()
                .map(|r| planned(p, r, &translation.properties))
        })
        .collect::<Result<_, _>>()?;
    let mut frame = plan_frame(&plans, &translation.properties.flips)?;
    let buffers = plan_buffers(
        &plans,
        &geometry_plans,
        &frame,
        &translation.buffers,
        &translation.properties.buffer_sizes,
    )?;
    skip_cleared_swap_back(&mut frame, &buffers, &plans);
    exchange_swap_back(&mut frame, &plans, &geometry_plans)?;
    let support = gpu.support();
    let mut buffer_formats = [None; MAX_COLOR_BUFFERS as usize];
    for buffer in &buffers {
        buffer_formats[buffer.index as usize] = Some(texture_format(buffer.format, support));
    }
    let mut storage_buffers = 0u32;
    let mut atomic_buffers = 0u32;
    for reflection in &reflections {
        for image in &reflection.storage_images {
            if let Some(b) = color_buffer_of(&image.name)
                && let Some(format) = buffer_formats[b as usize]
                && gpu.supports(format, TextureUsages::STORAGE_BINDING)
                && storage_format(image.format) == Some(format)
            {
                storage_buffers |= 1 << b;
                if image.atomic && gpu.supports_atomics(format) {
                    atomic_buffers |= 1 << b;
                }
            }
        }
    }
    let depth_format = TextureFormat::R32Float;
    let mut settings = pack_settings(&translation.programs);
    settings.shadow.culling = translation
        .properties
        .get("shadow.culling")
        .and_then(super::geometry::ShadowCulling::parse)
        .unwrap_or_default();
    // Iris takes a geometry stage on `shadow` for voxelization.
    settings.shadow.voxelizes = translation.programs.iter().any(|p| {
        p.enabled
            && p.program.name == "shadow"
            && p.stages
                .iter()
                .any(|s| s.stage == shaderpack::StageKind::Geometry)
    });

    check_cancelled(progress)?;
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let render_device = bevy::render::renderer::RenderDevice::from(gpu.device.clone());
    let loaded = crate::gpu_allocation::checked(&render_device, "shader pack load", 0, || {
        let key = content_key(pack, &config);
        let pipeline_cache = PipelineCacheFile::open(gpu, cache_dir, &key);
        let image_formats = image_formats(&translation.properties.images, &reflections, support);
        let atomic_images = translation
            .properties
            .images
            .iter()
            .zip(&image_formats)
            .map(|(custom, &format)| {
                gpu.supports_atomics(format)
                    && reflections
                        .iter()
                        .flat_map(|r| &r.storage_images)
                        .any(|image| image.atomic && image.name == custom.name)
            })
            .collect();
        let mut constants = ConstantBuilder::new(gpu, pack, &translation, vanilla, support);
        let mut depth_compare = [false; 3];
        let mut depth_sampled = [false; 3];
        let mut programs = Vec::with_capacity(all.len());
        // Per geometry program: its pipeline layout, vertex module and fragment stage.
        let mut geometry_stages = vec![];
        for (program, reflection) in all.iter().zip(&reflections) {
            check_cancelled(progress)?;
            let bindings = resolve_bindings(
                &mut constants,
                program.program.group,
                reflection,
                &buffer_formats,
                storage_buffers,
                depth_format,
                &translation.properties.images,
                &image_formats,
            )?;
            for source in &bindings.textures {
                match *source {
                    TextureSource::DepthCompare(i) => depth_compare[i] = true,
                    TextureSource::Depth { index, .. } => depth_sampled[index] = true,
                    _ => {}
                }
            }
            let sample_types: Vec<_> = bindings
                .textures
                .iter()
                .map(|s| s.sample_type(support))
                .collect();
            let entries = layout_entries(reflection, &sample_types)?;
            let layouts = bind_group_layouts(&gpu.device, &program.program.name, &entries);
            let layout_refs: Vec<Option<&wgpu::BindGroupLayout>> =
                layouts.iter().map(Option::as_ref).collect();
            let pipeline_layout =
                gpu.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some(&format!("shader pack {}", program.program.name)),
                        bind_group_layouts: &layout_refs,
                        immediate_size: 0,
                    });
            let stages: Vec<(StageKind, naga::Module)> = program
                .stages
                .iter()
                .filter_map(|s| s.result.as_ref().ok())
                .map(|c| (c.stage, c.module.clone()))
                .collect();
            let modules = shader_modules(&gpu.device, &program.program.name, &stages);
            if matches!(
                program.program.group,
                ProgramGroup::Gbuffers | ProgramGroup::Shadow
            ) {
                if program.program.group == ProgramGroup::Shadow {
                    for buffer in reflection.color_outputs.iter().filter_map(|o| o.buffer) {
                        let (slot, _) = constants.shadow_color(buffer)?;
                        if bindings.textures.iter().any(|source| matches!(source, TextureSource::Constant { index, .. } if *index == slot)) {
                            return Err(format!("{} samples shadowcolor{buffer} while writing it", program.program.name));
                        }
                    }
                }
                let vertex = modules
                    .into_iter()
                    .find(|(s, _)| *s == StageKind::Vertex)
                    .map(|(_, m)| m);
                let fragment = stages
                    .into_iter()
                    .find(|(s, _)| *s == StageKind::Fragment)
                    .map(|(_, m)| m);
                let (Some(vertex), Some(fragment)) = (vertex, fragment) else {
                    return Err(format!(
                        "{} needs a vertex and a fragment stage",
                        program.program.name
                    ));
                };
                geometry_stages.push(GeometryStage {
                    program: programs.len(),
                    layout: pipeline_layout,
                    vertex,
                    fragment,
                });
                programs.push(GpuProgram {
                    name: program.program.name.clone(),
                    group: program.program.group,
                    reflection: (*reflection).clone(),
                    pipeline: ProgramPipeline::Geometry,
                    layouts,
                    textures: bindings.textures,
                    images: bindings.images,
                    color_targets: vec![],
                    scale: None,
                });
                continue;
            }
            let module =
                |stage: StageKind| modules.iter().find(|(s, _)| *s == stage).map(|(_, m)| m);
            let entry = |stage: StageKind| {
                reflection
                    .entry_points
                    .iter()
                    .find(|(s, _)| *s == stage)
                    .map(|(_, e)| e.as_str())
            };
            let cache = pipeline_cache.as_ref().map(|c| &c.cache);
            let targets = color_targets(
                program.program.group,
                &program.program.name,
                reflection,
                &buffer_formats,
                &translation.properties.blends,
                None,
            );
            let pipeline = match reflection.kind {
                ProgramKind::Compute => {
                    let Some(module) = module(StageKind::Compute) else {
                        return Err(format!("{} has no compute stage", program.program.name));
                    };
                    ProgramPipeline::Compute(gpu.device.create_compute_pipeline(
                        &wgpu::ComputePipelineDescriptor {
                            label: Some(&program.program.name),
                            layout: Some(&pipeline_layout),
                            module,
                            entry_point: entry(StageKind::Compute),
                            compilation_options: Default::default(),
                            cache,
                        },
                    ))
                }
                ProgramKind::Render => {
                    let (Some(vertex), Some(fragment)) =
                        (module(StageKind::Vertex), module(StageKind::Fragment))
                    else {
                        return Err(format!(
                            "{} needs a vertex and a fragment stage",
                            program.program.name
                        ));
                    };
                    let no_targets = targets.iter().all(Option::is_none);
                    if no_targets && !stores(reflection) {
                        ProgramPipeline::Inert
                    } else {
                        let attributes = quad_attributes(reflection);
                        let pipeline =
                            gpu.device
                                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                                    label: Some(&program.program.name),
                                    layout: Some(&pipeline_layout),
                                    vertex: wgpu::VertexState {
                                        module: vertex,
                                        entry_point: entry(StageKind::Vertex),
                                        compilation_options: Default::default(),
                                        buffers: &[Some(quad_layout(&attributes))],
                                    },
                                    primitive: wgpu::PrimitiveState {
                                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                                        ..Default::default()
                                    },
                                    depth_stencil: None,
                                    multisample: Default::default(),
                                    fragment: Some(wgpu::FragmentState {
                                        module: fragment,
                                        entry_point: entry(StageKind::Fragment),
                                        compilation_options: Default::default(),
                                        targets: if no_targets {
                                            &MASKED_TARGET
                                        } else {
                                            targets.as_slice()
                                        },
                                    }),
                                    multiview_mask: None,
                                    cache,
                                });
                        if no_targets {
                            ProgramPipeline::StoreOnly(pipeline)
                        } else {
                            ProgramPipeline::Render(pipeline)
                        }
                    }
                }
            };
            programs.push(GpuProgram {
                name: program.program.name.clone(),
                group: program.program.group,
                reflection: (*reflection).clone(),
                pipeline,
                layouts,
                textures: bindings.textures,
                images: bindings.images,
                color_targets: targets.iter().map(Option::is_some).collect(),
                scale: translation
                    .properties
                    .scales
                    .iter()
                    .rev()
                    .find(|s| s.program == program.program.name)
                    .cloned(),
            });
        }
        let shadow_slots = constants.shadow_slots();
        check_cancelled(progress)?;
        let draws = geometry_draws(
            gpu,
            pipeline_cache.as_ref().map(|c| &c.cache),
            &selected_draws,
            &geometry_stages,
            &programs,
            &translation.properties,
            &effects,
            &buffer_formats,
            &shadow_slots,
        );
        // `shadowtex0` can be the attachment itself unless a shadow program samples it while
        // the pass draws.
        let shadowtex0 = shadow_slots.depth[0]
            .filter(|slot| {
                !draws.iter().filter(|d| d.kind.shadow()).any(|d| {
                    programs[d.program].textures.iter().any(
                        |source| matches!(source, TextureSource::Constant { index, .. } if index == slot),
                    )
                })
            })
            .map(|slot| (slot, constants.constant(slot)));
        let shadow = draws
            .iter()
            .any(|d| d.kind.shadow())
            .then(|| shadow_maps(gpu, shadow_slots, &translation.buffers, shadowtex0));
        let shadowcomp = (runnable.len() + geometry.len()..programs.len())
            .map(|program| PassStep {
                program,
                read_alt: 0,
                writes: vec![],
                mipmaps: vec![],
                mipmapped: 0,
            })
            .collect();
        let block_ids = block_ids.filter(|_| !draws.is_empty());
        let atlas_placeholder = constants
            .placeholder(ATLAS_FORMAT, shaderpack::TextureDimension::D2, false)?
            .0;
        let flag = |key: &str| translation.properties.get(key) == Some("true");
        let mipmap_formats: Vec<TextureFormat> = buffers
            .iter()
            .filter(|b| b.mipmapped)
            .filter_map(|b| buffer_formats[b.index as usize])
            .filter(|f| format_kind(*f) == Kind::Float)
            .collect();
        let shadow_depth_formats: Vec<_> = shadow
            .as_ref()
            .into_iter()
            .flat_map(|shadow| shadow.slots.float.iter().flatten())
            .map(|slot| constants.constant(*slot).texture.format())
            .collect();
        check_cancelled(progress)?;
        let host = host_pipelines(
            &gpu.device,
            &buffer_formats,
            depth_format,
            depth_compare.contains(&true) || !draws.is_empty(),
            super::targets::float_depths(draws.is_empty(), depth_sampled),
            &mipmap_formats,
            &shadow_depth_formats,
        );
        let shader_storage = storage_sizes(&reflections, &translation.properties)?;
        let max_uniform = reflections
            .iter()
            .flat_map(|r| &r.uniform_buffers)
            .map(|b| u64::from(b.min_size))
            .max()
            .unwrap_or(16)
            .max(16);
        let zero_uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader pack zero uniforms"),
            size: max_uniform.next_multiple_of(16),
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let quad_bytes = super::pipelines::quad_vertices();
        let quad = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader pack full-screen quad"),
            size: quad_bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&quad, 0, &quad_bytes);
        let filtering =
            SamplerClass::ALL.map(|class| gpu.device.create_sampler(&class_sampler(class)));
        let comparison = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader pack iris_samplerShadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let lightmap = constants.lightmap_slot();
        let constants = constants.finish();
        let loaded = PackGpu {
            name: name.to_string(),
            programs,
            frame,
            buffers,
            buffer_formats,
            storage_buffers,
            atomic_buffers,
            depth_format,
            depth_compare,
            depth_sampled,
            constants,
            custom_images: translation.properties.images.clone(),
            image_formats,
            atomic_images,
            shader_storage,
            settings,
            custom_uniforms,
            filtering,
            comparison,
            zero_uniforms,
            quad,
            host,
            terrain_layout: draws
                .iter()
                .find_map(|d| d.layout)
                .unwrap_or(super::geometry::TerrainLayout::WIDE),
            draws,
            shadowcomp,
            shadow,
            shadow_terrain: translation.properties.get("shadowTerrain") != Some("false"),
            shadow_translucent: translation.properties.get("shadowTranslucent") != Some("false"),
            block_ids,
            actor_ids,
            separate_ao: flag("separateAo"),
            old_lighting: flag("oldLighting"),
            atlas_placeholder,
            lightmap,
            effects,
        };
        Ok((loaded, pipeline_cache))
    });
    let validation_error = bevy::tasks::block_on(scope.pop());
    let (loaded, pipeline_cache) =
        loaded.ok_or_else(|| "shader pack load ran out of GPU memory".to_owned())??;
    if let Some(error) = validation_error {
        return Err(format!("the device rejected the pack: {error}"));
    }
    check_cancelled(progress)?;
    if let Some(cache) = pipeline_cache {
        cache.store();
    }
    bevy::log::info!(
        target: "cinnabar::shaderpack",
        pack = name, "ready after {:.0?}", started.elapsed()
    );
    Ok(loaded)
}

#[cfg(test)]
#[path = "loader_validation_tests.rs"]
mod validation_tests;
