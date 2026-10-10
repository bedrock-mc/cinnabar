//! Native sampling of the shared cached sky and captured reflection environment.

use super::frame::EnhancedFrameGpu;
use bevy::math::Vec4;
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

#[test]
fn rough_dielectric_reflection_keeps_foliage_tinted_and_conserves_energy() {
    let source = r#"
#import cinnabar::enhanced_environment::{environment_specular_weight, environment_diffuse_weight, environment_ray, environment_coordinate}
#import cinnabar::enhanced_view::{foliage_light, surface_visibility_weight, distribution_ggx}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,19>;
@compute @workgroup_size(1) fn regression() {
    let green=vec3(0.035,0.28,0.012);
    let rough=environment_specular_weight(vec3(0.04),0.9,0.05);
    let polished=environment_specular_weight(vec3(0.04),0.1,0.05);
    results[0]=vec4(rough, polished.x);
    let diffuse=environment_diffuse_weight(vec3(0.04),0.9,0.05,0.0);
    results[1]=vec4(green*diffuse+rough,1.0);
    var maximum=0.0;
    for (var r=0u;r<10u;r+=1u) {
        for(var v=0u;v<10u;v+=1u) {
            let roughness=f32(r)/9.0;
            let angle=f32(v)/9.0;
            let specular=environment_specular_weight(vec3(0.04,0.7,0.95),roughness,angle);
            let body=environment_diffuse_weight(vec3(0.04,0.7,0.95),roughness,angle,0.0);
            maximum=max(maximum,max(max((specular+body).x,(specular+body).y),(specular+body).z));
        }
    }
    results[2]=vec4(maximum,environment_diffuse_weight(green,0.5,0.5,1.0));
    results[3]=vec4(foliage_light(green,vec3(0.0,1.0,0.0),vec3(0.0,1.0,0.0),vec3(0.0,-1.0,0.0)),1.0);
    var coordinate_error=0.0;
    for(var face=0u;face<6u;face+=1u) {
        let uv=vec2(0.23,0.71);
        let ray=environment_ray(uv,face);
        let coordinate=environment_coordinate(ray);
        // Each face stays observable, matching the production array-layer dispatch.
        results[7u+face]=vec4(coordinate,1.0);
        results[13u+face]=vec4(ray,1.0);
        coordinate_error=max(coordinate_error,length(coordinate-vec3(uv,f32(face))));
    }
    results[4]=vec4(coordinate_error,surface_visibility_weight(10.0,10.18),surface_visibility_weight(10.0,10.0),1.0);
    results[5]=vec4(environment_specular_weight(green,0.8,0.5),1.0);
    let roughness=0.16;
    let expected_peak_scale=3.14159265359*pow(roughness,4.0);
    results[6]=vec4(distribution_ggx(1.0,roughness)*expected_peak_scale,
        distribution_ggx(1.0,0.045),distribution_ggx(0.0,roughness),1.0);
}
"#;
    let Some(values) = super::post_regressions::execute(source, 19) else {
        return;
    };
    assert!(
        values[0][0] > 0.01 && values[0][0] < 0.08,
        "rough leaves must not become mirrors"
    );
    assert!(
        values[0][3] > values[0][0] * 5.0,
        "polished and rough materials retain different grazing response"
    );
    assert!(
        values[1][1] > values[1][0] * 3.0 && values[1][1] > values[1][2] * 4.0,
        "white illumination preserves foliage chroma"
    );
    assert!(values[2][0] <= 1.00001);
    assert_eq!(&values[2][1..], &[0.0; 3], "metals have no diffuse lobe");
    assert!(
        values[3][1] > values[3][0] * 7.0,
        "transmission keeps authored absorption colour"
    );
    assert!(
        values[4][0] < 0.00001,
        "cube sampling agrees with capture camera axes: {values:?}"
    );
    for (face, (coordinate, ray)) in values[7..13].iter().zip(&values[13..19]).enumerate() {
        for (actual, expected) in coordinate[..3].iter().zip([0.23, 0.71, face as f32]) {
            assert!(
                (actual - expected).abs() < 0.00001,
                "face {face}: {coordinate:?}"
            );
        }
        let squared_length = ray[..3]
            .iter()
            .map(|channel| channel * channel)
            .sum::<f32>();
        assert!(
            (squared_length - 1.0).abs() < 0.00001,
            "face {face}: {ray:?}"
        );
    }
    assert!(
        values[4][1] < 0.002,
        "thin contacts must not blend an unrelated depth plane"
    );
    assert!((values[4][2] - 1.0).abs() < 0.00001);
    assert!(
        values[5][1] > values[5][0] * 3.0,
        "metallic reflection follows albedo"
    );
    assert!(
        (values[6][0] - 1.0).abs() < 0.0001,
        "polished GGX retains its normalized peak"
    );
    assert!(
        values[6][1].is_finite() && values[6][1] > 10000.0,
        "minimum roughness remains finite without flattening the lobe"
    );
}

const SAMPLE: &str = r#"
#import cinnabar::enhanced_view::{reflection_sky, reflection_environment, surface_indirect}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,11>;
@compute @workgroup_size(1) fn regression() {
    let directions = array<vec3<f32>,6>(
        vec3(1.0,0.0,0.0), vec3(-1.0,0.0,0.0),
        vec3(0.0,1.0,0.0), vec3(0.0,-1.0,0.0),
        vec3(0.0,0.0,1.0), vec3(0.0,0.0,-1.0));
    results[0] = vec4(reflection_sky(normalize(vec3(0.3,0.6,-0.2))),1.0);
    results[1] = vec4(reflection_environment(vec3(32.0,0.0,0.0),directions[0],0.4,0.25),1.0);
    results[2] = vec4(reflection_environment(vec3(0.0),directions[2],0.0,0.35),1.0);
    for (var face=0u; face<6u; face+=1u) {
        results[3u+face] = vec4(reflection_environment(vec3(0.0),directions[face],0.6,0.0),1.0);
    }
    results[9] = vec4(surface_indirect(vec3(0.0),directions[2],0.35,vec3(0.02,0.03,0.04),vec2(0.0)),1.0);
    results[10] = vec4(surface_indirect(vec3(32.0,0.0,0.0),directions[2],0.25,vec3(0.02,0.03,0.04),vec2(0.0)),1.0);
}
"#;

#[test]
fn native_reflections_use_cached_sky_and_preserve_full_capture_confidence() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for Enhanced cached reflection environment");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("cached reflection regression device"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("cached reflection regression device");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production Enhanced reflection sampling"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(SAMPLE, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("cached reflection sampling regression"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let captured: [[u8; 4]; 6] = [
        [64, 16, 32, 255],
        [32, 96, 48, 255],
        [80, 40, 120, 255],
        [24, 64, 96, 255],
        [112, 48, 16, 255],
        [40, 128, 72, 255],
    ];
    let cached_sky = [17_u8, 73, 151, 255];
    let diffuse = captured.map(|colour| [colour[0] / 2, colour[1] / 2, colour[2] / 2, colour[3]]);
    let sky_diffuse = [12_u8, 43, 76, 255];
    let mut texels = captured.to_vec();
    texels.extend(diffuse);
    texels.extend([sky_diffuse, cached_sky, cached_sky]);
    let extent = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: texels.len() as u32,
    };
    let environment = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("six captured faces and independent cached sky fixture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        environment.as_image_copy(),
        bytemuck::cast_slice(&texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        extent,
    );
    let environment_view = environment.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let empty_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("unused reflection fixture group"),
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[],
    });
    let byte_count = 11 * std::mem::size_of::<[f32; 4]>() as u64;
    let local_sources = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("empty production local source storage"),
        contents: &vec![0; super::local_lights::LOCAL_LIGHT_BUFFER_BYTES],
        usage: wgpu::BufferUsages::STORAGE,
    });
    let local_tiles = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("empty local contribution tile"),
        contents: bytemuck::cast_slice(&[1u32, 1, super::local_lights::TILE_SIDE, 0, 0, 0, 0, 0]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let spatial_field = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("disabled spatial irradiance fixture"),
        contents: &[0; super::indirect::INDIRECT_MIN_BYTES as usize],
        usage: wgpu::BufferUsages::STORAGE,
    });
    for probe_radius in [0.0, 16.0, -1.0] {
        let mut frame = EnhancedFrameGpu::zeroed();
        frame.probe = Vec4::new(0.0, 0.0, 0.0, probe_radius);
        frame.camera_time.y = 64.0;
        frame.celestial = Vec4::new(0.0, 1.0, 0.0, 0.0);
        frame.atmosphere.x = 1.0;
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("production cached reflection frame"),
            contents: bytemuck::bytes_of(&frame),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cached reflection GPU outputs"),
            size: byte_count,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cached reflection results readback"),
            size: byte_count,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let (scattering, scattering_sampler) =
            super::multiple_scattering::fixture(&device, &mut encoder);
        let result_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cached reflection regression output"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 22,
                    resource: wgpu::BindingResource::TextureView(&scattering),
                },
                wgpu::BindGroupEntry {
                    binding: 23,
                    resource: wgpu::BindingResource::Sampler(&scattering_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 31,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let environment_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("production reflection frame, sampler and environment slots"),
            layout: &pipeline.get_bind_group_layout(2),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&environment_view),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: local_sources.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: local_tiles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: spatial_field.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &result_group, &[]);
            pass.set_bind_group(1, &empty_group, &[]);
            pass.set_bind_group(2, &environment_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, byte_count);
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        let values: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
        close_colour(values[0], cached_sky, 1.0);
        close_colour(values[1], cached_sky, 0.25);
        if probe_radius <= 0.0 {
            close_colour(values[2], cached_sky, 0.35);
            for value in &values[3..9] {
                close_colour(*value, cached_sky, 0.0);
            }
        } else {
            close_colour(values[2], captured[2], 1.0);
            for (value, face) in values[3..9].iter().zip(captured) {
                close_colour(*value, face, 1.0);
            }
        }
        for channel in 0..3 {
            let block = [0.02, 0.03, 0.04][channel];
            let near = f32::from(sky_diffuse[channel]) / 255.0 * 0.35;
            assert!((values[9][channel] - near - block).abs() < 0.00001);
            let far = f32::from(sky_diffuse[channel]) / 255.0 * 0.25;
            assert!((values[10][channel] - far - block).abs() < 0.00001);
        }
    }
}

fn close_colour(actual: [f32; 4], encoded: [u8; 4], illumination: f32) {
    for channel in 0..3 {
        let expected = f32::from(encoded[channel]) / 255.0 * illumination;
        assert!(
            (actual[channel] - expected).abs() < 1.0e-5,
            "cached reflection channel {channel}: actual {actual:?}, expected {expected}"
        );
    }
}
