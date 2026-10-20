//! Turns a shader pack into GPU-ready programs on a worker thread.
//!
//! The worker opens the pack, translates the selected dimension, logs every program's
//! diagnostics, plans buffers and passes, and builds pipelines and pack-wide textures with
//! the shared device. Translation takes tens of milliseconds; driver pipeline compilation
//! dominates the load, so the cache keeps the backend pipeline cache (Vulkan) in a private
//! directory, keyed by the pack's content hash, the translation settings and the adapter.

use super::host_pipelines::{HostPipelines, host_pipelines};
use super::pipeline_cache::PipelineCacheFile;
use super::pipelines::{
    GpuContext, Kind, MASKED_TARGET, bind_group_layouts, class_sampler, color_targets, format_kind,
    layout_entries, quad_attributes, quad_layout, shader_modules, storage_format, stores,
    texture_format,
};
use super::plan::{
    BufferPlan, FramePlan, MAX_COLOR_BUFFERS, PlannedProgram, color_buffer_of, exchange_swap_back,
    plan_buffers, plan_frame, skip_cleared_swap_back,
};
use super::resources::bindings::resolve_bindings;
use super::resources::vanilla::VanillaTextures;
use super::resources::{
    ATLAS_FORMAT, ConstantBuilder, ConstantTextures, ImageSource, TextureSource, image_formats,
};
use super::uniforms::PackSettings;
use sha2::{Digest, Sha256};
use shaderpack::{
    BlendMode, BufferId, CustomUniform, Environment, ProgramGroup, ProgramKind, ProgramReflection,
    ProgramTranslation, SamplerBinding, SamplerClass, ScaleRule, ShaderPack, ShaderProperties,
    StageKind, TranslateConfig,
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
    pub custom_uniforms: Vec<CustomUniform>,
    /// One filtering sampler per [`SamplerClass`], in [`SamplerClass::ALL`] order.
    pub filtering: [wgpu::Sampler; SamplerClass::ALL.len()],
    pub comparison: wgpu::Sampler,
    pub zero_uniforms: wgpu::Buffer,
    pub quad: wgpu::Buffer,
    pub host: HostPipelines,
    /// `entityId` and item ids per Bedrock identifier.
    pub actor_ids: Arc<super::ids::ActorIdMaps>,
    /// Pack-wide placeholder bound for the block atlas, which full-screen programs do not see.
    pub atlas_placeholder: usize,
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
/// programs (`gbuffers_*`, `shadow*`, `shadowcomp*`) are not run: the Vanilla scene stands in
/// for them.
fn runs(group: ProgramGroup) -> bool {
    matches!(
        group,
        ProgramGroup::Prepare
            | ProgramGroup::Deferred
            | ProgramGroup::Composite
            | ProgramGroup::Final
    )
}

/// Logs a program's diagnostics; failures of programs this host runs are warnings.
fn log_program(pack: &str, program: &ProgramTranslation) {
    let required = program.enabled && runs(program.program.group);
    for diagnostic in program.all_diagnostics() {
        let location = diagnostic
            .location
            .as_ref()
            .map(|l| format!("{}:{}", l.file, l.line))
            .unwrap_or_default();
        if required {
            bevy::log::warn!(
                target: "cinnabar::shaderpack",
                pack, program = %program.program.name, %location,
                "{:?}: {}", diagnostic.phase, diagnostic.message
            );
        } else {
            bevy::log::debug!(
                target: "cinnabar::shaderpack",
                pack, program = %program.program.name, %location,
                "{:?}: {}", diagnostic.phase, diagnostic.message
            );
        }
    }
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
pub(crate) fn translate_config(pack: &ShaderPack, dimension: &str) -> TranslateConfig {
    let environment = Environment {
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
    };
    TranslateConfig {
        folder: pack.dimension_folder(dimension, &environment),
        environment,
        ..TranslateConfig::default()
    }
}

/// Destination buffer per fragment output location.
fn outputs_by_location(reflection: &ProgramReflection) -> Vec<Option<u32>> {
    let len = reflection
        .color_outputs
        .iter()
        .map(|o| o.location as usize + 1)
        .max()
        .unwrap_or(0);
    let mut outputs = vec![None; len];
    for output in &reflection.color_outputs {
        outputs[output.location as usize] = output.buffer;
    }
    outputs
}

/// Whether some function of `module` can discard a fragment.
fn discards(module: &naga::Module) -> bool {
    fn kills(block: &naga::Block) -> bool {
        block.iter().any(|statement| match statement {
            naga::Statement::Kill => true,
            naga::Statement::Block(inner) => kills(inner),
            naga::Statement::If { accept, reject, .. } => kills(accept) || kills(reject),
            naga::Statement::Switch { cases, .. } => cases.iter().any(|c| kills(&c.body)),
            naga::Statement::Loop {
                body, continuing, ..
            } => kills(body) || kills(continuing),
            _ => false,
        })
    }
    module.functions.iter().any(|(_, f)| kills(&f.body))
        || module.entry_points.iter().any(|e| kills(&e.function.body))
}

fn planned(
    program: &ProgramTranslation,
    reflection: &ProgramReflection,
    properties: &ShaderProperties,
) -> PlannedProgram {
    let name = program.program.name.as_str();
    let mipmaps = program
        .consts
        .iter()
        .filter(|c| c.value.trim() == "true")
        .filter_map(|c| c.name.strip_suffix("MipmapEnabled"))
        .filter_map(|b| match BufferId::parse(b) {
            Some(BufferId::Color(i)) if i < MAX_COLOR_BUFFERS => Some(i),
            _ => None,
        })
        .collect();
    let outputs = if reflection.kind == ProgramKind::Render {
        outputs_by_location(reflection)
    } else {
        vec![]
    };
    let whole = properties.scales.iter().all(|s| s.program != name)
        && !program
            .stages
            .iter()
            .filter_map(|s| s.result.as_ref().ok())
            .any(|c| c.stage == StageKind::Fragment && discards(&c.module));
    let blends = |buffer: u32| {
        let rule = |per_buffer: bool| {
            properties.blends.iter().rev().find(|r| {
                r.program == name
                    && match r.buffer.as_deref() {
                        Some(b) => per_buffer && color_buffer_of(b) == Some(buffer),
                        None => !per_buffer,
                    }
            })
        };
        rule(true)
            .or_else(|| rule(false))
            .is_some_and(|r| r.mode != BlendMode::Off)
    };
    let partial = outputs
        .iter()
        .flatten()
        .filter(|&&b| !whole || blends(b))
        .fold(0u32, |mask, b| mask | (1 << b));
    PlannedProgram {
        name: name.to_owned(),
        group: program.program.group,
        kind: program.program.kind,
        outputs,
        reads: reflection
            .textures
            .iter()
            .map(|t| t.name.as_str())
            .chain(reflection.storage_images.iter().map(|i| i.name.as_str()))
            .filter_map(color_buffer_of)
            .collect(),
        mipmaps,
        partial,
    }
}

/// Opens, translates and builds `path` for `dimension` (a Java dimension id); `vanilla`
/// answers the pack's `minecraft:` texture references.
pub(crate) fn load(
    path: &Path,
    dimension: &str,
    cache_dir: Option<&Path>,
    gpu: &GpuContext,
    vanilla: &VanillaTextures,
) -> Result<PackGpu, String> {
    let pack = ShaderPack::open(path).map_err(|e| e.to_string())?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    build(&pack, &name, dimension, cache_dir, gpu, vanilla)
}

/// The enabled programs this host runs, in frame order, with their plans; an error names
/// every such program that failed to translate.
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
                plans.push(planned(program, reflection, &translation.properties));
                runnable.push(program);
            }
            None => failed.push(program.program.name.clone()),
        }
    }
    if failed.is_empty() {
        Ok((runnable, plans))
    } else {
        Err(format!("required programs failed: {}", failed.join(", ")))
    }
}

/// Translates and builds an opened pack.
pub(crate) fn build(
    pack: &ShaderPack,
    name: &str,
    dimension: &str,
    cache_dir: Option<&Path>,
    gpu: &GpuContext,
    vanilla: &VanillaTextures,
) -> Result<PackGpu, String> {
    let config = translate_config(pack, dimension);
    let started = std::time::Instant::now();
    let translation = pack.translate(&config);
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
    let (_, actor_ids) =
        super::ids::id_tables(pack.id_maps(&config.environment, &config.options), name);
    bevy::log::info!(
        target: "cinnabar::shaderpack",
        pack = name, folder = %config.folder,
        programs = translation.programs.len(), runnable = runnable.len(),
        "translated in {:.0?}", started.elapsed()
    );
    let reflections: Vec<&ProgramReflection> = runnable
        .iter()
        .filter_map(|p| p.reflection.as_ref())
        .collect();
    let mut frame = plan_frame(&plans, &translation.properties.flips);
    let buffers = plan_buffers(
        &plans,
        &[],
        &frame,
        &translation.buffers,
        &translation.properties.buffer_sizes,
    );
    skip_cleared_swap_back(&mut frame, &buffers, &plans);
    exchange_swap_back(&mut frame, &plans, &[]);
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
    let depth_format = if support.float32_filterable {
        TextureFormat::R32Float
    } else {
        bevy::log::warn!(
            target: "cinnabar::shaderpack",
            "device cannot filter 32-bit floats; depth buffers use 16-bit precision"
        );
        TextureFormat::R16Float
    };
    let settings = pack_settings(&translation.programs);

    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
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
    let mut programs = Vec::with_capacity(runnable.len());
    for (program, reflection) in runnable.iter().zip(&reflections) {
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
        let pipeline_layout = gpu
            .device
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
        let module = |stage: StageKind| modules.iter().find(|(s, _)| *s == stage).map(|(_, m)| m);
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
    let atlas_placeholder = constants
        .placeholder(ATLAS_FORMAT, shaderpack::TextureDimension::D2, false)?
        .0;
    let mipmap_formats: Vec<TextureFormat> = buffers
        .iter()
        .filter(|b| b.mipmapped)
        .filter_map(|b| buffer_formats[b.index as usize])
        .filter(|f| format_kind(*f) == Kind::Float)
        .collect();
    let host = host_pipelines(
        &gpu.device,
        &buffer_formats,
        depth_format,
        depth_compare.contains(&true),
        super::targets::float_depths(depth_sampled),
        &mipmap_formats,
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
    let filtering = SamplerClass::ALL.map(|class| gpu.device.create_sampler(&class_sampler(class)));
    let comparison = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("shader pack iris_samplerShadow"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let constants = constants.finish();
    if let Some(error) = bevy::tasks::block_on(scope.pop()) {
        return Err(format!("the device rejected the pack: {error}"));
    }
    if let Some(cache) = &pipeline_cache {
        cache.store();
    }
    bevy::log::info!(
        target: "cinnabar::shaderpack",
        pack = name, "ready after {:.0?}", started.elapsed()
    );
    Ok(PackGpu {
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
        custom_uniforms: translation.properties.custom_uniforms.clone(),
        filtering,
        comparison,
        zero_uniforms,
        quad,
        host,
        actor_ids,
        atlas_placeholder,
    })
}

/// Storage bindings: the reflected minimum, raised to the pack's complete requested words.
/// Resolves whole-buffer binding sizes, rejecting requests whose alignment would overflow.
fn storage_sizes(
    reflections: &[&ProgramReflection],
    properties: &shaderpack::ShaderProperties,
) -> Result<Vec<(u32, u64)>, String> {
    let mut sizes: Vec<(u32, u64)> = vec![];
    for buffer in reflections.iter().flat_map(|r| &r.storage_buffers) {
        let declared = properties
            .get(&format!("bufferObject.{}", buffer.binding))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0);
        let size = storage_binding_size(buffer.min_size, declared)
            .map_err(|error| format!("storage binding {}: {error}", buffer.binding))?;
        match sizes.iter_mut().find(|(b, _)| *b == buffer.binding) {
            Some((_, s)) => *s = (*s).max(size),
            None => sizes.push((buffer.binding, size)),
        }
    }
    Ok(sizes)
}

/// Settings declared by the enabled programs in the pack.
fn pack_settings(programs: &[ProgramTranslation]) -> PackSettings {
    PackSettings::read(
        programs
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.consts),
    )
}

/// Binding size in complete storage words, at least the reflected prefix.
fn storage_binding_size(minimum: u32, declared: u64) -> Result<u64, String> {
    let minimum = u64::from(minimum).max(4).next_multiple_of(4);
    let size = minimum.max(declared);
    let aligned = size.checked_next_multiple_of(4).ok_or("size overflows")?;
    Ok(aligned - if size.is_multiple_of(4) { 0 } else { 4 })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program with one pack-wide setting, independently of host execution support.
    fn setting_program(group: ProgramGroup, enabled: bool, value: &str) -> ProgramTranslation {
        ProgramTranslation {
            program: shaderpack::ProgramSource {
                name: String::new(),
                group,
                kind: ProgramKind::Render,
                folder: String::new(),
                stages: vec![],
            },
            enabled,
            forced: vec![],
            stages: vec![],
            diagnostics: vec![],
            reflection: None,
            draw_buffers: vec![],
            consts: vec![shaderpack::ConstDirective {
                ty: "float".into(),
                name: "sunPathRotation".into(),
                value: value.into(),
            }],
        }
    }

    #[test]
    fn pack_settings_include_enabled_geometry_and_shadow_programs() {
        let programs = [
            setting_program(ProgramGroup::Gbuffers, true, "30.0"),
            setting_program(ProgramGroup::Shadow, true, "45.0"),
            setting_program(ProgramGroup::Composite, false, "90.0"),
        ];
        assert_eq!(pack_settings(&programs).sun_path_rotation, 45.0);
    }

    #[test]
    fn storage_binding_lengths_keep_complete_declared_words() {
        for declared in [20, 21, 23] {
            assert_eq!(storage_binding_size(4, declared).unwrap(), 20);
        }
        assert_eq!(storage_binding_size(16, 4).unwrap(), 16);
        assert!(storage_binding_size(4, u64::MAX).is_err());
    }
}
