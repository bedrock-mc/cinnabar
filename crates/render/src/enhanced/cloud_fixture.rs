//! Uses production GPU generation for native cloud regression inputs.
use super::cloud_noise;
pub(super) fn generate(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let desc = cloud_noise::texture_descriptor();
    let texture = device.create_texture(&desc);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production cloud noise"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(
                &cloud_noise::shader_source(include_str!("cloud_noise.wgsl")),
                &[],
            )
            .into(),
        ),
    });
    let pipeline = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let generate = pipeline("generate_noise");
    let filter = pipeline("filter_noise");
    let mips: Vec<_> = (0..desc.mip_level_count)
        .map(|mip| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D3),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    for level in 0..mips.len() {
        let pipeline = if level == 0 { &generate } else { &filter };
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&mips[level]),
        }];
        if level > 0 {
            entries.push(wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&mips[level - 1]),
            });
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud generation fixture"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let side = (cloud_noise::CLOUD_NOISE_SIZE >> level)
            .max(1)
            .div_ceil(cloud_noise::CLOUD_NOISE_WORKGROUP);
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(side, side, side);
    }
    (
        texture.create_view(&Default::default()),
        device.create_sampler(&cloud_noise::sampler_descriptor()),
    )
}
