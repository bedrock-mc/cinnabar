//! Per-view render targets of a loaded pack and the bind groups that read them.
//!
//! Like Enhanced's persistent post targets, everything here lives as long as the view keeps
//! its size and pack: colour buffers (main and, when the plan ping-pongs them, alternate
//! textures with optional mip chains), the `depthtex*` copies, the screen `final` draws,
//! custom images, storage buffers, per-program uniform buffers and every pass's bind groups.
//! A resize or pack change replaces the whole set.

use super::loader::PackGpu;
use super::pipelines::SCREEN_FORMAT;
use super::plan::{BufferReads, MAX_COLOR_BUFFERS, PassStep};
use super::resources::{ImageSource, TextureSource};
use shaderpack::ProgramKind;

pub(super) mod admission;
mod image_clears;
use std::sync::Arc;
use wgpu::{Device, TextureFormat, TextureUsages, TextureView};

/// One colour buffer's textures.
pub(crate) struct ColorBuffer {
    pub size: [u32; 2],
    textures: Vec<wgpu::Texture>,
    /// Every mip, for sampling; per side.
    pub sampled: Vec<TextureView>,
    /// Mip 0 only, for attachments and storage; per side.
    pub attachment: Vec<TextureView>,
    /// Each mip level alone, per side (empty without mipmaps).
    pub levels: Vec<Vec<TextureView>>,
}

impl ColorBuffer {
    /// Allocates colour sides and their full-chain, attachment and per-level views.
    fn new(
        device: &Device,
        label: &str,
        format: TextureFormat,
        size: [u32; 2],
        mips: u32,
        sides: usize,
        usage: TextureUsages,
    ) -> Self {
        let textures: Vec<_> = (0..sides)
            .map(|side| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&format!(
                        "{label}{}",
                        if side == 1 { " alternate" } else { "" }
                    )),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: mips,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            })
            .collect();
        let level = |texture: &wgpu::Texture, mip| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        Self {
            size,
            sampled: textures
                .iter()
                .map(|texture| texture.create_view(&Default::default()))
                .collect(),
            attachment: textures.iter().map(|texture| level(texture, 0)).collect(),
            levels: if mips > 1 {
                textures
                    .iter()
                    .map(|texture| (0..mips).map(|mip| level(texture, mip)).collect())
                    .collect()
            } else {
                vec![]
            },
            textures,
        }
    }

    /// A sampled copy with the same allocated mip chain as this colour buffer.
    fn snapshot(&self, device: &Device, label: &str) -> Self {
        Self::new(
            device,
            label,
            self.texture(0).format(),
            self.size,
            self.texture(0).mip_level_count(),
            1,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        )
    }

    /// Copies every mip of the selected side into its sampled snapshot.
    fn copy_snapshot(&self, encoder: &mut wgpu::CommandEncoder, side: usize, snapshot: &Self) {
        for (mip, size) in snapshot_levels(self.size, self.texture(side).mip_level_count()) {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    mip_level: mip,
                    ..self.texture(side).as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    mip_level: mip,
                    ..snapshot.texture(0).as_image_copy()
                },
                size,
            );
        }
    }

    /// The whole chain when mips are visible to this pass, otherwise only its base level.
    fn sampled_view(&self, side: usize, mipmapped: bool) -> &TextureView {
        if mipmapped {
            &self.sampled[side]
        } else {
            &self.attachment[side]
        }
    }

    /// Index of the main (`false`) or alternate (`true`) texture; buffers without an
    /// alternate always use main.
    pub(crate) fn side(&self, alternate: bool) -> usize {
        usize::from(alternate && self.textures.len() > 1)
    }

    pub(crate) fn texture(&self, side: usize) -> &wgpu::Texture {
        &self.textures[side]
    }
}

/// Which float `depthtexN` copies a pack allocates: only those some program samples.
pub(crate) fn float_depths(sampled: [bool; 3]) -> [bool; 3] {
    sampled
}

/// A texture with its default view.
pub(crate) struct Target {
    pub texture: wgpu::Texture,
    pub view: TextureView,
}

fn target(
    device: &Device,
    label: &str,
    format: TextureFormat,
    size: [u32; 2],
    usage: TextureUsages,
) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Target { texture, view }
}

/// Every view-sized resource of a pack.
pub(crate) struct ViewTargets {
    pub screen_size: [u32; 2],
    pub buffers: Vec<Option<ColorBuffer>>,
    /// Float `depthtexN` copies where [`float_depths`] allocates them.
    pub depth: [Option<Target>; 3],
    pub depth_compare: [Option<Target>; 3],
    pub screen: Target,
    pub images: Vec<(wgpu::Texture, TextureView)>,
    image_clears: Option<image_clears::ImageClears>,
    pub storage: Vec<(u32, wgpu::Buffer)>,
    /// Per colour buffer, the copy sampled by programs that also write it as an image.
    pub snapshots: Vec<Option<ColorBuffer>>,
    /// Per custom image, the copy sampled by programs that also write it as an image.
    pub image_snapshots: Vec<Option<Target>>,
    /// Flips each time [`Self::swap_sides`] exchanges the textures of the pack's
    /// `frame.exchange` buffers; indexes the bind groups built over that arrangement.
    pub parity: usize,
}

/// Bind groups of one pass or draw, by group index.
pub(crate) type Groups = [Option<wgpu::BindGroup>; 4];

/// The buffers of the pack's `frame.exchange` that program `program` binds, whose
/// arrangement its bind groups depend on.
pub(crate) fn exchanged_by(pack: &PackGpu, program: usize) -> Vec<u32> {
    let program = &pack.programs[program];
    pack.frame
        .exchange
        .iter()
        .copied()
        .filter(|&b| {
            program
                .textures
                .iter()
                .any(|t| matches!(t, TextureSource::Buffer { index, .. } if *index == b))
                || program
                    .images
                    .iter()
                    .any(|i| matches!(i, ImageSource::Buffer(index) if *index == b))
        })
        .collect()
}

impl ViewTargets {
    /// Allocates exact requests, returning device validation or allocation failures to the host.
    pub(crate) fn new(
        device: &Device,
        pack: &PackGpu,
        screen_size: [u32; 2],
    ) -> Result<Self, String> {
        admission::validate(pack, screen_size, &device.limits())?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
        let targets =
            crate::gpu_allocation::checked(&render_device, "shader pack view targets", 0, || {
                Self::allocate(device, pack, screen_size)
            });
        if let Some(error) = bevy::tasks::block_on(scope.pop()) {
            return Err(error.to_string());
        }
        targets.ok_or_else(|| "shader pack view targets ran out of GPU memory".to_owned())
    }

    /// Creates preflighted targets inside the caller's validation and allocation scopes.
    fn allocate(device: &Device, pack: &PackGpu, screen_size: [u32; 2]) -> Self {
        let mut buffers: Vec<Option<ColorBuffer>> = (0..MAX_COLOR_BUFFERS).map(|_| None).collect();
        for plan in &pack.buffers {
            let Some(format) = pack.buffer_formats[plan.index as usize] else {
                continue;
            };
            let size = plan.extent.resolve(screen_size);
            let mips = if plan.mipmapped {
                size[0].max(size[1]).max(1).ilog2() + 1
            } else {
                1
            };
            let mut usage = TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST;
            if pack.storage_buffers & (1 << plan.index) != 0 {
                usage |= TextureUsages::STORAGE_BINDING;
            }
            if pack.atomic_buffers & (1 << plan.index) != 0 {
                usage |= TextureUsages::STORAGE_ATOMIC;
            }
            let sides = if plan.alternate { 2 } else { 1 };
            buffers[plan.index as usize] = Some(ColorBuffer::new(
                device,
                &format!("shader pack colortex{}", plan.index),
                format,
                size,
                mips,
                sides,
                usage,
            ));
        }
        let sampled = TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING;
        let allocated = float_depths(pack.depth_sampled);
        let depth = std::array::from_fn(|i| {
            allocated[i].then(|| {
                target(
                    device,
                    &format!("shader pack depthtex{i}"),
                    pack.depth_format,
                    screen_size,
                    sampled,
                )
            })
        });
        let depth_compare = std::array::from_fn(|i| {
            pack.depth_compare[i].then(|| {
                target(
                    device,
                    &format!("shader pack depthtex{i} comparison"),
                    TextureFormat::Depth32Float,
                    screen_size,
                    sampled | TextureUsages::COPY_DST,
                )
            })
        });
        let screen = target(
            device,
            "shader pack screen",
            SCREEN_FORMAT,
            screen_size,
            sampled,
        );
        let image_snapshotted: Vec<bool> = (0..pack.custom_images.len())
            .map(|index| {
                pack.programs
                    .iter()
                    .any(|program| program.image_sampled_while_written(index))
            })
            .collect();
        let images: Vec<(wgpu::Texture, TextureView)> = pack
            .custom_images
            .iter()
            .zip(&pack.image_formats)
            .zip(&pack.atomic_images)
            .zip(&image_snapshotted)
            .map(|(((image, format), &atomic), &snapshotted)| {
                let (dimension, size) = admission::image_extent(image.size, screen_size);
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&image.name),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: size[2],
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension,
                    format: *format,
                    usage: TextureUsages::STORAGE_BINDING
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_DST
                        | if atomic {
                            TextureUsages::STORAGE_ATOMIC
                        } else {
                            TextureUsages::empty()
                        }
                        | if snapshotted {
                            TextureUsages::COPY_SRC
                        } else {
                            TextureUsages::empty()
                        },
                    view_formats: &[],
                });
                let view = texture.create_view(&Default::default());
                (texture, view)
            })
            .collect();
        let image_snapshots = images
            .iter()
            .zip(&image_snapshotted)
            .map(|((image, _), &snapshotted)| {
                snapshotted.then(|| {
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("shader pack custom image snapshot"),
                        size: image.size(),
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: image.dimension(),
                        format: image.format(),
                        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&Default::default());
                    Target { texture, view }
                })
            })
            .collect();
        let storage = pack
            .shader_storage
            .iter()
            .map(|(binding, size)| {
                (
                    *binding,
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("shader pack storage buffer"),
                        size: *size,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                )
            })
            .collect();
        let snapshotted = pack
            .programs
            .iter()
            .fold(0, |bits, p| bits | p.sampled_while_written());
        let snapshots = buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| {
                let buffer = buffer
                    .as_ref()
                    .filter(|_| snapshotted & (1 << index) != 0)?;
                Some(buffer.snapshot(device, &format!("shader pack colortex{index} snapshot")))
            })
            .collect();
        Self {
            screen_size,
            buffers,
            depth,
            depth_compare,
            screen,
            images,
            image_clears: pack
                .custom_images
                .iter()
                .any(|image| image.clear)
                .then(|| image_clears::ImageClears::new(device)),
            storage,
            snapshots,
            image_snapshots,
            parity: 0,
        }
    }

    /// Clears only custom images that request per-frame clearing, before any programs run.
    pub(crate) fn clear_images(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        images: &[shaderpack::CustomImage],
    ) {
        let Some(clears) = &self.image_clears else {
            return;
        };
        for (image, (texture, _)) in images.iter().zip(&self.images) {
            if image.clear {
                clears.record(encoder, texture);
            }
        }
    }

    /// Exchanges the main and alternate textures of `buffers` at the end of a frame, so a
    /// buffer that ended flipped starts the next one with its last side as main, as Iris's
    /// copy of that side back to main does (a full-screen copy per buffer per frame).
    pub(crate) fn swap_sides(&mut self, buffers: &[u32]) {
        if buffers.is_empty() {
            return;
        }
        self.exchange_textures(buffers);
        self.parity ^= 1;
    }

    fn exchange_textures(&mut self, buffers: &[u32]) {
        for &index in buffers {
            let Some(buffer) = self.buffers[index as usize]
                .as_mut()
                .filter(|b| b.textures.len() > 1)
            else {
                continue;
            };
            buffer.textures.swap(0, 1);
            buffer.sampled.swap(0, 1);
            buffer.attachment.swap(0, 1);
            if !buffer.levels.is_empty() {
                buffer.levels.swap(0, 1);
            }
        }
    }

    /// `build` over both arrangements of the textures of `buffers` (some of the exchanged
    /// ones), indexed by parity; built once when `buffers` is empty.
    pub(crate) fn per_parity<T: Clone>(
        &mut self,
        buffers: &[u32],
        mut build: impl FnMut(&Self) -> T,
    ) -> [T; 2] {
        let current = build(self);
        if buffers.is_empty() {
            return [current.clone(), current];
        }
        self.exchange_textures(buffers);
        let other = build(self);
        self.exchange_textures(buffers);
        if self.parity == 0 {
            [current, other]
        } else {
            [other, current]
        }
    }

    /// Copies the side a pass reads of each buffer it samples while writing into its
    /// snapshot, and each custom image it samples while writing; [`pass_bind_groups`] binds
    /// the snapshots for those samplers.
    pub(crate) fn take_snapshots(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        program: &super::loader::GpuProgram,
        read_alt: u32,
    ) {
        let mask = program.sampled_while_written();
        for (index, snapshot) in self.snapshots.iter().enumerate() {
            let (Some(snapshot), Some(buffer)) = (snapshot, &self.buffers[index]) else {
                continue;
            };
            if mask & (1 << index) == 0 {
                continue;
            }
            let side = buffer.side(read_alt & (1 << index) != 0);
            buffer.copy_snapshot(encoder, side, snapshot);
        }
        for (index, snapshot) in self.image_snapshots.iter().enumerate() {
            let Some(snapshot) = snapshot else {
                continue;
            };
            if !program.image_sampled_while_written(index) {
                continue;
            }
            let image = &self.images[index].0;
            encoder.copy_texture_to_texture(
                image.as_image_copy(),
                snapshot.texture.as_image_copy(),
                image.size(),
            );
        }
    }

    /// The view a texture binding reads during a pass with `reads`: a buffer side whose mips
    /// are visible reads every level, else its base level only. Full-screen passes read the
    /// atlas placeholder for the block atlas.
    fn texture_view<'a>(
        &'a self,
        pack: &'a PackGpu,
        source: TextureSource,
        reads: BufferReads,
    ) -> &'a TextureView {
        match source {
            TextureSource::Buffer { index, .. } => {
                let buffer = self.buffers[index as usize]
                    .as_ref()
                    .expect("planned buffers are allocated");
                let side = buffer.side(reads.alt & (1 << index) != 0);
                buffer.sampled_view(side, reads.mipmapped & (1 << index) != 0)
            }
            TextureSource::Depth { index, .. } => self.depth_view(index),
            TextureSource::DepthCompare(index) => {
                &self.depth_compare[index]
                    .as_ref()
                    .expect("comparison depth is allocated when a program binds it")
                    .view
            }
            TextureSource::Image { index, .. } => &self.images[index].1,
            TextureSource::Constant { index, .. } => &pack.constants.textures[index].view,
            TextureSource::Atlas(_) => &pack.constants.textures[pack.atlas_placeholder].view,
        }
    }

    /// The float `depthtex{index}` copy; [`Self::depth`] says when each exists.
    pub(crate) fn depth_view(&self, index: usize) -> &TextureView {
        &self.depth[index]
            .as_ref()
            .expect("a depthtex copy is allocated wherever it is read")
            .view
    }
}

/// Builds the bind groups of program `program` for a pass whose buffer reads are `reads`.
pub(crate) fn pass_bind_groups<'a>(
    device: &Device,
    pack: &'a PackGpu,
    targets: &'a ViewTargets,
    uniforms: Option<&'a wgpu::Buffer>,
    program: usize,
    reads: BufferReads,
) -> [Option<wgpu::BindGroup>; 4] {
    let entries = pass_bind_entries(pack, targets, uniforms, program, reads);
    let program = &pack.programs[program];
    std::array::from_fn(|group| {
        let layout = program.layouts[group].as_ref()?;
        Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&program.name),
            layout,
            entries: &entries[group],
        }))
    })
}

/// The entries of `pass_bind_groups`, per group.
fn pass_bind_entries<'a>(
    pack: &'a PackGpu,
    targets: &'a ViewTargets,
    uniforms: Option<&'a wgpu::Buffer>,
    program: usize,
    reads: BufferReads,
) -> [Vec<wgpu::BindGroupEntry<'a>>; 4] {
    let program = &pack.programs[program];
    let reflection = &program.reflection;
    let mut entries: [Vec<wgpu::BindGroupEntry<'a>>; 4] = Default::default();
    let mut push = |group: u32, binding: u32, resource: wgpu::BindingResource<'a>| {
        if let Some(e) = entries.get_mut(group as usize) {
            e.push(wgpu::BindGroupEntry { binding, resource });
        }
    };
    if let (Some(block), Some(buffer)) = (&reflection.uniforms, uniforms) {
        push(block.group, block.binding, buffer.as_entire_binding());
    }
    for block in &reflection.uniform_buffers {
        push(
            block.group,
            block.binding,
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &pack.zero_uniforms,
                offset: 0,
                size: std::num::NonZeroU64::new(u64::from(block.min_size).next_multiple_of(16)),
            }),
        );
    }
    for block in &reflection.storage_buffers {
        if let Some((_, buffer)) = targets.storage.iter().find(|(b, _)| *b == block.binding) {
            push(block.group, block.binding, buffer.as_entire_binding());
        }
    }
    for sampler in &reflection.samplers {
        push(
            sampler.group,
            sampler.binding,
            wgpu::BindingResource::Sampler(pack.sampler(sampler)),
        );
    }
    let snapshotted = program.sampled_while_written();
    for (texture, source) in reflection.textures.iter().zip(&program.textures) {
        let view = match *source {
            TextureSource::Buffer { index, .. } if snapshotted & (1 << index) != 0 => targets
                .snapshots[index as usize]
                .as_ref()
                .expect("snapshots are allocated for every such buffer")
                .sampled_view(0, reads.mipmapped & (1 << index) != 0),
            TextureSource::Image { index, .. } if program.image_sampled_while_written(index) => {
                &targets.image_snapshots[index]
                    .as_ref()
                    .expect("snapshots are allocated for every such image")
                    .view
            }
            source => targets.texture_view(pack, source, reads),
        };
        push(
            texture.group,
            texture.binding,
            wgpu::BindingResource::TextureView(view),
        );
    }
    for (image, source) in reflection.storage_images.iter().zip(&program.images) {
        let view = match *source {
            ImageSource::Buffer(index) => {
                let buffer = targets.buffers[index as usize]
                    .as_ref()
                    .expect("planned buffers are allocated");
                &buffer.attachment[buffer.side(reads.alt & (1 << index) != 0)]
            }
            ImageSource::Image(index) => &targets.images[index].1,
            ImageSource::Constant(index) => &pack.constants.textures[index].view,
        };
        push(
            image.group,
            image.binding,
            wgpu::BindingResource::TextureView(view),
        );
    }
    entries
}

/// One mipmapped buffer side's downsample bind groups: one per level after the first,
/// reading the level above.
pub(crate) type Downsample = (u32, usize, Vec<wgpu::BindGroup>);

/// A view's uniform buffer per program and bind groups per pass. Everything binding a
/// buffer's textures is built for both [`ViewTargets::parity`] arrangements and indexed by
/// it.
pub(crate) struct ViewBindings {
    pub uniforms: Vec<Option<wgpu::Buffer>>,
    pub prepare: Vec<[Groups; 2]>,
    pub post: Vec<[Groups; 2]>,
    /// Per mipmapped buffer and side.
    pub downsample: [Vec<Downsample>; 2],
    /// Per view target format, as `PackGpu::host.present`.
    pub present: [Vec<(TextureFormat, wgpu::BindGroup)>; 2],
}

fn uniform_buffer(device: &Device, name: &str, block: &shaderpack::UniformBlock) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("shader pack {name} uniforms")),
        size: u64::from(block.size).max(16).next_multiple_of(16),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl ViewBindings {
    pub(crate) fn new(device: &Device, pack: &Arc<PackGpu>, targets: &mut ViewTargets) -> Self {
        let uniforms: Vec<Option<wgpu::Buffer>> = pack
            .programs
            .iter()
            .map(|p| {
                let block = p.reflection.uniforms.as_ref()?;
                Some(uniform_buffer(device, &p.name, block))
            })
            .collect();
        let mut groups = |steps: &[PassStep]| {
            steps
                .iter()
                .map(|step| {
                    targets.per_parity(&exchanged_by(pack, step.program), |targets| {
                        pass_bind_groups(
                            device,
                            pack,
                            targets,
                            uniforms[step.program].as_ref(),
                            step.program,
                            step.reads(),
                        )
                    })
                })
                .collect()
        };
        let prepare = groups(&pack.frame.prepare);
        let post = groups(&pack.frame.post);
        let exchanged = |keep: &dyn Fn(u32) -> bool| -> Vec<u32> {
            pack.frame
                .exchange
                .iter()
                .copied()
                .filter(|&b| keep(b))
                .collect()
        };
        let mipmapped = exchanged(&|b| pack.buffers.iter().any(|p| p.index == b && p.mipmapped));
        let downsample = targets.per_parity(&mipmapped, |t| downsample_groups(device, pack, t));
        let presents_colortex0 = !pack
            .programs
            .iter()
            .any(|p| p.group == shaderpack::ProgramGroup::Final);
        let presented = exchanged(&|b| presents_colortex0 && b == 0);
        let present = targets.per_parity(&presented, |t| present_groups(device, pack, t));
        Self {
            uniforms,
            prepare,
            post,
            downsample,
            present,
        }
    }
}

/// The downsample bind groups of every mipmapped buffer side.
fn downsample_groups(device: &Device, pack: &PackGpu, targets: &ViewTargets) -> Vec<Downsample> {
    let mut downsample = vec![];
    for plan in pack.buffers.iter().filter(|b| b.mipmapped) {
        let Some(buffer) = targets.buffers[plan.index as usize].as_ref() else {
            continue;
        };
        let Some(format) = pack.buffer_formats[plan.index as usize] else {
            continue;
        };
        let Some((_, pipeline)) = pack.host.downsample.iter().find(|(f, _)| *f == format) else {
            continue;
        };
        let layout = pipeline.get_bind_group_layout(0);
        for (side, levels) in buffer.levels.iter().enumerate() {
            let groups = levels
                .iter()
                .take(levels.len().saturating_sub(1))
                .map(|source| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("shader pack downsample"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(source),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::Sampler(&pack.host.linear),
                            },
                        ],
                    })
                })
                .collect();
            downsample.push((plan.index, side, groups));
        }
    }
    downsample
}

/// The present bind group per view target format: the screen `final` drew, else
/// `colortex0`'s main texture.
fn present_groups(
    device: &Device,
    pack: &PackGpu,
    targets: &ViewTargets,
) -> Vec<(TextureFormat, wgpu::BindGroup)> {
    let present_source = if pack
        .programs
        .iter()
        .any(|p| p.group == shaderpack::ProgramGroup::Final)
    {
        &targets.screen.view
    } else {
        let buffer = targets.buffers[0]
            .as_ref()
            .expect("colortex0 is always planned");
        &buffer.sampled[0]
    };
    pack.host
        .present
        .iter()
        .map(|(format, pipeline)| {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader pack present"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(present_source),
                }],
            });
            (*format, group)
        })
        .collect()
}

/// Whether a program dispatches compute work.
pub(crate) fn is_compute(pack: &PackGpu, step: &PassStep) -> bool {
    pack.programs[step.program].reflection.kind == ProgramKind::Compute
}

/// The mip extents a colour snapshot must preserve, including rectangular tail levels.
fn snapshot_levels(size: [u32; 2], mips: u32) -> impl Iterator<Item = (u32, wgpu::Extent3d)> {
    (0..mips).map(move |mip| {
        (
            mip,
            wgpu::Extent3d {
                width: (size[0] >> mip).max(1),
                height: (size[1] >> mip).max(1),
                depth_or_array_layers: 1,
            },
        )
    })
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshots_copy_every_allocated_mip() {
        let sizes: Vec<_> = snapshot_levels([16, 4], 5)
            .map(|(level, size)| (level, size.width, size.height))
            .collect();
        assert_eq!(
            sizes,
            [(0, 16, 4), (1, 8, 2), (2, 4, 1), (3, 2, 1), (4, 1, 1)]
        );
    }
    #[test]
    fn gpu_snapshots_keep_the_source_chain_and_per_pass_mip_visibility() {
        let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
            return;
        };
        let source = ColorBuffer::new(
            &gpu.device,
            "snapshot source",
            TextureFormat::Rgba8Unorm,
            [16, 4],
            5,
            1,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
        );
        let snapshot = source.snapshot(&gpu.device, "snapshot copy");
        assert_eq!(snapshot.texture(0).mip_level_count(), 5);
        assert!(std::ptr::eq(
            snapshot.sampled_view(0, false),
            &snapshot.attachment[0]
        ));
        assert!(std::ptr::eq(
            snapshot.sampled_view(0, true),
            &snapshot.sampled[0]
        ));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.copy_snapshot(&mut encoder, 0, &snapshot);
        gpu.queue.submit([encoder.finish()]);
    }
}
