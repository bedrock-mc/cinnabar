//! `centerDepthSmooth`, as Iris's `CenterDepthSampler`: the depth buffer's centre texel
//! once the opaque world (terrain, entities, block entities and the effects drawn before
//! `deferred`) has drawn, before the hand and translucents, moved each frame towards the new
//! sample by the share `centerDepthHalflife` gives the frame time. Sampling and smoothing
//! run on the GPU into a one-float buffer that is copied into the uniform blocks of the
//! full-screen programs declaring `uniform float centerDepthSmooth` or a float custom
//! uniform that aliases it, so nothing is read back.
//! Iris binds the value only for full-screen passes: `deferred`, `composite` and `final` read
//! this frame's sample, earlier passes the previous one, and geometry programs read 0.

use shaderpack::{CustomUniforms, ProgramGroup, ScalarType, UniformBlock, ValueType};
use wgpu::util::DeviceExt;
use wgpu::{BindGroup, Buffer, CommandEncoder, ComputePipeline, Device, Queue, TextureView};

/// The uniform's name.
pub(crate) const UNIFORM: &str = "centerDepthSmooth";

/// The value before the first sample: the cleared depth the sky leaves.
const INITIAL: f32 = 1.0;

/// Bytes of the per-frame share uniform (one float, padded for uniform binding rules).
const SHARE_BYTES: u64 = 16;

/// Which depth a pack's centre sample reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepthSource {
    /// The pack's geometry depth (OpenGL window depth, bottom-up).
    Geometry,
    /// The native scene depth copied in for packs without geometry programs.
    Scene,
}

impl DepthSource {
    /// The `blit.wgsl` compute entry point sampling this source.
    pub(super) fn entry(self) -> &'static str {
        match self {
            DepthSource::Geometry => "center_depth_geometry",
            DepthSource::Scene => "center_depth_scene",
        }
    }
}

/// A sampling pipeline together with the source its inferred layout accepts.
pub(crate) struct DepthPipeline {
    pub(super) source: DepthSource,
    pub(super) pipeline: ComputePipeline,
}

/// One full-screen program's `centerDepthSmooth` member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reader {
    /// Index into the pack's programs (and the view's per-program uniform buffers).
    pub program: usize,
    /// Byte offset of the member in the program's uniform block.
    pub offset: u64,
}

/// The full-screen programs that read `centerDepthSmooth`, by when they run relative to
/// the frame's sample.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CenterDepthReaders {
    /// Programs running before the sample (`shadowcomp`, `prepare`): the previous value.
    pub before: Vec<Reader>,
    /// `deferred`, `composite` and `final` programs: this frame's value.
    pub after: Vec<Reader>,
}

impl CenterDepthReaders {
    /// Plans the readers from each program's group and, for full-screen programs, its loose
    /// uniform block (`None` for geometry programs). Float custom aliases share the
    /// source value; arrays and declarations of other types do not.
    pub(crate) fn plan<'a>(
        programs: impl IntoIterator<Item = (ProgramGroup, Option<&'a UniformBlock>)>,
        custom: &CustomUniforms,
    ) -> Self {
        let mut readers = Self::default();
        let aliases = custom.float_aliases(UNIFORM);
        for (program, (group, block)) in programs.into_iter().enumerate() {
            let Some(block) = block else {
                continue;
            };
            for member in block.members.iter().filter(|member| {
                (member.name == UNIFORM || aliases.contains(&member.name.as_str()))
                    && member.array.is_none()
                    && member.ty == ValueType::Scalar(ScalarType::Float)
            }) {
                let reader = Reader {
                    program,
                    offset: u64::from(member.offset),
                };
                match group {
                    ProgramGroup::Deferred | ProgramGroup::Composite | ProgramGroup::Final => {
                        readers.after.push(reader);
                    }
                    _ => readers.before.push(reader),
                }
            }
        }
        readers
    }

    /// Whether no program reads the value, so nothing is sampled.
    pub(crate) fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty()
    }
}

/// One view's smoothed centre depth and the bindings that sample it.
pub(crate) struct CenterDepth {
    /// The host's sampling pipeline for the pack's [`DepthSource`].
    pipeline: ComputePipeline,
    /// The smoothed depth: one float the compute pass reads and writes.
    value: Buffer,
    /// This frame's share of the new sample.
    share: Buffer,
    /// Over the geometry depth; `None` for packs without geometry programs.
    geometry: Option<BindGroup>,
    /// Over the native scene depth and the view's depth parameters, with the depth view it
    /// was built for.
    scene: Option<(TextureView, BindGroup)>,
}

impl CenterDepth {
    /// A view's sampler over `pipeline` (the host's [`DepthSource`] entry), bound to the
    /// view's geometry depth when the pack draws geometry.
    pub(crate) fn new(
        device: &Device,
        pipeline: &DepthPipeline,
        geometry_depth: Option<&TextureView>,
    ) -> Self {
        let geometry_depth = match pipeline.source {
            DepthSource::Geometry => Some(geometry_depth.expect("geometry sampler needs depth")),
            DepthSource::Scene => None,
        };
        let pipeline = &pipeline.pipeline;
        let value = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shader pack centerDepthSmooth"),
            contents: bytemuck::bytes_of(&INITIAL),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let share = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader pack centerDepthSmooth share"),
            size: SHARE_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let geometry = geometry_depth.map(|depth| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader pack centerDepthSmooth geometry"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: value.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: share.as_entire_binding(),
                    },
                ],
            })
        });
        Self {
            pipeline: pipeline.clone(),
            value,
            share,
            geometry,
            scene: None,
        }
    }

    /// Writes the share of the gap to the new sample that this frame closes: 1 on the
    /// first frame (and after a restart), so smoothing starts from the first sample.
    pub(crate) fn write_share(&self, queue: &Queue, share: f32) {
        queue.write_buffer(&self.share, 0, bytemuck::bytes_of(&[share, 0.0, 0.0, 0.0]));
    }

    /// Copies the current value into the uniform blocks of `readers`, overwriting the
    /// placeholder the CPU packed there this frame.
    pub(crate) fn publish(
        &self,
        encoder: &mut CommandEncoder,
        readers: &[Reader],
        uniforms: &[Option<Buffer>],
    ) {
        for reader in readers {
            if let Some(buffer) = &uniforms[reader.program] {
                encoder.copy_buffer_to_buffer(
                    &self.value,
                    0,
                    buffer,
                    reader.offset,
                    size_of::<f32>() as u64,
                );
            }
        }
    }

    /// Samples the geometry depth's centre and publishes the result to `readers`.
    pub(crate) fn sample_geometry(
        &self,
        encoder: &mut CommandEncoder,
        readers: &[Reader],
        uniforms: &[Option<Buffer>],
    ) {
        if let Some(group) = &self.geometry {
            self.sample(encoder, group, readers, uniforms);
        }
    }

    /// Samples the copied geometry depth for a geometry pack, or the native scene depth
    /// otherwise (`parameters` locate and convert it), and publishes to `readers`.
    pub(crate) fn sample_scene(
        &mut self,
        device: &Device,
        encoder: &mut CommandEncoder,
        readers: &[Reader],
        uniforms: &[Option<Buffer>],
        depth: &TextureView,
        parameters: &Buffer,
    ) {
        // Scene copying also fills the geometry attachment for geometry packs. Keep
        // using the source selected with the pipeline, even before terrain is ready.
        if self.geometry.is_some() {
            self.sample_geometry(encoder, readers, uniforms);
            return;
        }
        if self.scene.as_ref().is_none_or(|(view, _)| view != depth) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shader pack centerDepthSmooth scene"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: parameters.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.value.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: self.share.as_entire_binding(),
                    },
                ],
            });
            self.scene = Some((depth.clone(), group));
        }
        let (_, group) = self.scene.as_ref().expect("built above");
        self.sample(encoder, group, readers, uniforms);
    }

    /// One invocation smooths the value towards the centre sample, then `readers` get it.
    fn sample(
        &self,
        encoder: &mut CommandEncoder,
        group: &BindGroup,
        readers: &[Reader],
        uniforms: &[Option<Buffer>],
    ) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("shader pack centerDepthSmooth"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        self.publish(encoder, readers, uniforms);
    }
}

#[cfg(test)]
#[path = "center_depth_tests.rs"]
mod tests;
