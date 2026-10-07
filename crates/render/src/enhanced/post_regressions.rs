//! Executes production lighting, exposure and display functions on a native adapter.

use crate::shader_source;
use wgpu::util::DeviceExt as _;

pub(super) fn execute(source: &str, result_count: usize) -> Option<Vec<[f32; 4]>> {
    execute_internal(source, result_count, false)
}

pub(super) fn execute_with_clouds(source: &str, result_count: usize) -> Option<Vec<[f32; 4]>> {
    execute_internal(source, result_count, true)
}

fn execute_internal(source: &str, result_count: usize, clouds: bool) -> Option<Vec<[f32; 4]>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for Enhanced lighting regressions");
        return None;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Enhanced lighting regression device"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("Enhanced lighting regression device");
    let source = shader_source::composed(source, &[]);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production Enhanced regression functions"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Enhanced lighting regressions"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let byte_count = (result_count * std::mem::size_of::<[f32; 4]>()) as u64;
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Enhanced regression outputs"),
        contents: &vec![0; byte_count as usize],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Enhanced regression readback"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let noise = clouds.then(|| super::cloud_fixture::generate(&device, &mut encoder));
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 31,
        resource: output.as_entire_binding(),
    }];
    if let Some((view, sampler)) = &noise {
        entries.push(wgpu::BindGroupEntry {
            binding: 13,
            resource: wgpu::BindingResource::TextureView(view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 14,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
    }
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Enhanced regression output binding"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, byte_count);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("regression readback")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("regression GPU completion");
    let values = bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    Some(values)
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1.0e-4,
        "GPU result {actual} differs from {expected}"
    );
}

#[test]
fn occlusion_does_not_dim_emission_or_skip_bright_materials() {
    let source = r#"
#import cinnabar::enhanced_radiance::compose_surface_lighting
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    let indirect=vec3(4.0,2.0,1.0);
    let direct=vec3(6.0,4.0,2.0);
    let emission=vec3(12.0,9.0,3.0);
    results[0]=vec4(compose_surface_lighting(indirect,direct,emission,vec2(0.25,0.0)),1.0);
    results[1]=vec4(compose_surface_lighting(indirect,direct,emission,vec2(0.0,0.0)),1.0);
    results[2]=vec4(compose_surface_lighting(indirect,direct,vec3(0.0),vec2(0.0,1.0)),1.0);
    results[3]=vec4(compose_surface_lighting(indirect,direct,vec3(0.0),vec2(1.0,0.0)),1.0);
}

"#;
    let Some(values) = execute(source, 4) else {
        return;
    };
    for (actual, expected) in values[0][..3].iter().zip([13.0, 9.5, 3.25]) {
        close(*actual, expected);
    }
    for (actual, expected) in values[1][..3].iter().zip([12.0, 9.0, 3.0]) {
        close(*actual, expected);
    }
    for (actual, expected) in values[2][..3].iter().zip([6.0, 4.0, 2.0]) {
        close(*actual, expected);
    }
    for (actual, expected) in values[3][..3].iter().zip([4.0, 2.0, 1.0]) {
        close(*actual, expected);
    }
}

#[test]
fn untextured_models_receive_light_without_becoming_emitters() {
    let source = r#"
#import cinnabar::enhanced_radiance::{untextured_material_mer,compose_surface_lighting}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    let material=untextured_material_mer();
    let albedo=vec3(0.9,0.75,0.6);
    let emission=albedo*material.y;
    results[0]=vec4(material,1.0);
    results[1]=vec4(compose_surface_lighting(vec3(0.6,0.5,0.4),vec3(4.0,3.0,2.0),emission,vec2(1.0)),1.0);
    results[2]=vec4(compose_surface_lighting(vec3(0.6,0.5,0.4),vec3(4.0,3.0,2.0),emission,vec2(0.0)),1.0);
}
"#;
    let Some(values) = execute(source, 3) else {
        return;
    };
    close(values[0][1], 0.0);
    assert!(values[0][2] > 0.0, "fallback retains a finite roughness");
    assert!(
        values[1][0] > 4.0,
        "ordinary models still receive incident light"
    );
    for channel in 0..3 {
        close(values[2][channel], 0.0);
    }
}

#[test]
fn night_exposure_preserves_darkness_and_keeps_a_finite_gain() {
    let mut source = include_str!("exposure.wgsl").to_owned();
    source.push_str(
        r#"
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    let noon=exposure_policy(0.1,1.0,0.0);
    let night=exposure_policy(0.1,0.0,0.0);
    let darkness=exposure_policy(0.000001,0.0,0.0);
    let lamps=exposure_policy(16.0,0.0,0.0);
    results[0]=vec4(noon.x,night.x,darkness.x,lamps.x);
    results[1]=vec4(noon.y,night.y,darkness.z,lamps.y);
    let unlit=exposure_policy(0.008,0.0,0.0);
    results[2]=vec4(unlit.x*0.008,0.0,0.0,1.0);
}
"#,
    );
    let Some(values) = execute(&source, 3) else {
        return;
    };
    assert!(
        values[0][1] < values[0][0] * 0.5,
        "night should remain darker"
    );
    assert!(
        values[1][1] < values[1][0] * 0.5,
        "night midpoint must remain lower"
    );
    close(values[0][2], values[1][2]);
    assert!(
        values[0][2] < 4.0,
        "dark skies must not receive unbounded gain"
    );
    assert!(values[0][3] * 16.0 > 1.0, "lamps must retain HDR radiance");
    assert!(
        values[2][0] > 0.022,
        "unlit terrain must retain readable display radiance"
    );
}

#[test]
fn diagnostic_colours_bypass_exposure_and_filmic_grading() {
    let mut source = include_str!("post.wgsl").to_owned();
    source.push_str(
        r#"
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,3>;
@compute @workgroup_size(1) fn regression(){
    let colour=vec3(0.85,0.12,0.08);
    results[0]=vec4(display_radiance(colour,0.01,true),1.0);
    results[1]=vec4(display_radiance(colour,100.0,true),1.0);
    results[2]=vec4(display_radiance(colour*4.0,1.0,false),1.0);
}
"#,
    );
    let Some(values) = execute(&source, 3) else {
        return;
    };
    for channel in 0..3 {
        close(values[0][channel], [0.85, 0.12, 0.08][channel]);
        close(values[1][channel], values[0][channel]);
    }
    assert!((values[2][1] - values[0][1]).abs() > 0.01);
}

#[test]
fn display_preserves_night_detail_and_highlight_hue_with_a_finite_shoulder() {
    let mut source = include_str!("post.wgsl").to_owned();
    source.push_str(
        r#"
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    results[0]=vec4(display_radiance(vec3(0.003,0.01,0.02),1.0,false),1.0);
    results[1]=vec4(display_radiance(vec3(0.4,0.8,0.2),1.0,false),1.0);
    results[2]=vec4(display_radiance(vec3(2.0),1.0,false),1.0);
    results[3]=vec4(display_radiance(vec3(4.0),1.0,false),1.0);
    results[4]=vec4(display_radiance(vec3(16.0,4.0,1.0),1.0,false),1.0);
}
"#,
    );
    let Some(values) = execute(&source, 5) else {
        return;
    };
    for (actual, expected) in values[0][..3].iter().zip([0.003, 0.01, 0.02]) {
        close(*actual, expected);
    }
    close(values[1][1] / values[1][0], 2.0);
    close(values[1][1] / values[1][2], 4.0);
    assert!(values[3][0] > values[2][0] && values[3][0] < 1.0);
    assert!(
        values
            .iter()
            .flatten()
            .all(|value| value.is_finite() && *value >= 0.0 && *value <= 1.0)
    );
    assert!(values[4][0] > values[4][1] && values[4][1] > values[4][2]);
}

#[test]
fn temporal_sharpening_is_bounded_and_leaves_flat_regions_and_high_contrast_edges_unchanged() {
    let source = r#"
#import cinnabar::enhanced_temporal::{temporal_sharpen,temporal_clip}
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    results[0]=vec4(temporal_sharpen(vec3(0.4),vec3(0.4),vec3(0.4),vec3(0.4),vec3(0.4)),1.0);
    results[1]=vec4(temporal_sharpen(vec3(0.5),vec3(0.4),vec3(0.4),vec3(0.6),vec3(0.45)),1.0);
    results[2]=vec4(temporal_sharpen(vec3(1.0),vec3(0.0),vec3(0.0),vec3(1.0),vec3(1.0)),1.0);
    results[3]=vec4(temporal_sharpen(vec3(0.0),vec3(0.0),vec3(1.0),vec3(1.0),vec3(0.0)),1.0);
    results[4]=vec4(temporal_clip(vec3(3.0,1.0,-1.0),vec3(0.0),vec3(1.0)),1.0);
}

"#;
    let Some(values) = execute(source, 5) else {
        return;
    };
    close(values[0][0], 0.4);
    assert!(
        values[1][0] > 0.5 && values[1][0] <= 0.52,
        "low-contrast detail receives bounded recovery"
    );
    close(values[2][0], 1.0);
    close(values[3][0], 0.0);
    assert!(
        values[4][..3]
            .iter()
            .all(|value| *value >= 0.0 && *value <= 1.0)
    );
    close((values[4][0] - 0.5) / (values[4][2] - 0.5), -5.0 / 3.0);
}

#[test]
fn transparency_reactivity_rejects_chroma_changes_without_resetting_unchanged_opaque_surfaces() {
    let source = r#"
#import cinnabar::enhanced_temporal::temporal_transparency_reactivity
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,1>;
@compute @workgroup_size(1) fn regression(){
    let opaque=vec3(0.4,0.5,0.3);
    results[0]=vec4(temporal_transparency_reactivity(opaque,opaque),
        temporal_transparency_reactivity(opaque*1.01,opaque),
        temporal_transparency_reactivity(vec3(0.2,0.5,0.5),opaque),
        temporal_transparency_reactivity(vec3(0.0),opaque));
}
"#;
    let Some(values) = execute(source, 1) else {
        return;
    };
    close(values[0][0], 0.0);
    close(values[0][1], 0.0);
    close(values[0][2], 1.0);
    close(values[0][3], 1.0);
}
