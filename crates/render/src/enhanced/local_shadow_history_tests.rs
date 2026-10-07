use super::*;
use wgpu::util::DeviceExt as _;

#[test]
fn lamp_response_depends_on_elapsed_time_instead_of_refresh_rate() {
    let integrate = |hz: u32| {
        let mut visibility = 1.0;
        for _ in 0..hz / 10 {
            visibility *= 1.0 - update_weight(1.0 / hz as f32);
        }
        visibility
    };
    let reference = integrate(60);
    for hz in [30, 120, 240] {
        assert!((reference - integrate(hz)).abs() < 0.00001);
    }
    assert!(update_weight(1.0 / 60.0) > 0.0 && update_weight(1.0 / 60.0) < 0.4);
    assert_eq!(update_weight(f32::NAN), 1.0);
}

#[test]
fn native_receiver_history_softens_repeated_occlusion_without_reusing_disocclusions() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU for dynamic local shadow history");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut source = include_str!("local_shadow_history.wgsl").to_owned();
    source.push_str(&format!(
        "\nconst TEST_UPDATE_WEIGHT:f32={};\n",
        update_weight(1.0 / 60.0)
    ));
    source.push_str(r#"
@group(0) @binding(31) var<storage,read_write> result:array<vec4<f32>,8>;
@compute @workgroup_size(1) fn regression(){
    let parameters=vec4(1.0,TEST_UPDATE_WEIGHT,0.0,0.0);
    result[0]=vec4(local_shadow_mix(vec2(0.0),vec4(1.0,1.0,1.0,8.0),8.0,parameters),0.0,0.0);
    result[1]=vec4(local_shadow_mix(vec2(0.0),vec4(1.0,1.0,1.0,8.0),8.0,vec4(0.0,parameters.y,0.0,0.0)),0.0,0.0);
    result[2]=vec4(local_shadow_mix(vec2(0.0),vec4(1.0,1.0,1.0,20.0),8.0,parameters),0.0,0.0);
    result[3]=vec4(local_shadow_mix(vec2(0.0),vec4(1.0,1.0,0.0,8.0),8.0,parameters),0.0,0.0);
    var value=vec2(0.5);var low=1.0;var high=0.0;var jump=0.0;
    for(var step=0u;step<120u;step+=1u){
        let next=local_shadow_mix(vec2(f32(step%2u)),vec4(value,1.0,8.0),8.0,parameters);
        jump=max(jump,abs(next.x-value.x));value=next;low=min(low,value.x);high=max(high,value.x);
    }
    result[4]=vec4(low,high,jump,value.x);
    result[5]=vec4(local_shadow_mix(vec2(1.0),vec4(1.0,1.0,1.0,8.0),8.0,parameters),0.0,0.0);
    var recovered=vec2(0.0);
    for(var step=0u;step<18u;step+=1u){recovered=local_shadow_mix(vec2(1.0),vec4(recovered,1.0,8.0),8.0,parameters);}
    result[6]=vec4(recovered,0.0,0.0);
    result[7]=vec4(parameters.y,0.0,0.0,0.0);
}
"#);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production dynamic receiver visibility"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(&source, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("dynamic local shadow transition fixture"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 128],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 128,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 31,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 128);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    let values =
        bytemuck::cast_slice::<u8, [f32; 4]>(&readback.slice(..).get_mapped_range()).to_vec();
    readback.unmap();
    assert!(
        values[0][0] > 0.5 && values[0][0] < 1.0,
        "first darkening frame retains partial visibility instead of switching off"
    );
    for value in &values[1..4] {
        assert_eq!(
            &value[..2],
            &[0.0, 0.0],
            "cuts, absent history and newly exposed receivers use current occlusion"
        );
    }
    assert!(
        values[4][0] > 0.3 && values[4][1] < 0.7 && values[4][2] < 0.4,
        "rapid caster crossings must not flash between zero and full light"
    );
    assert_eq!(&values[5][..2], &[1.0, 1.0]);
    assert!(
        values[6][0] > 0.99,
        "an unobstructed light clears the short visibility history"
    );
    assert!((values[7][0] - update_weight(1.0 / 60.0)).abs() < 0.00001);
}

#[test]
fn native_preparation_reuses_unchanged_policy_and_rejects_source_camera_and_submission_cuts() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU for local shadow preparation caching");
        return;
    };
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let device = RenderDevice::from(device);
    let queue = RenderQueue(std::sync::Arc::new(
        bevy::render::renderer::WgpuWrapper::new(queue),
    ));
    let mut history = LocalShadowHistory::new(&device, [8, 8]);
    let prepare = |history: &mut LocalShadowHistory, submitted, camera, source| {
        history.submitted.store(submitted, Ordering::Relaxed);
        history.prepare(&queue, camera, source, 1.0 / 60.0);
        history.uploaded_parameters.unwrap()[0]
    };
    assert_eq!(prepare(&mut history, false, true, 1), 0.0);
    assert_eq!(prepare(&mut history, true, true, 1), 1.0);
    let uploads = history.parameter_uploads;
    let buffer = history.parameters.id();
    for _ in 0..120 {
        assert_eq!(prepare(&mut history, true, true, 1), 1.0);
    }
    assert_eq!(
        history.parameter_uploads, uploads,
        "unchanged policy uploads nothing"
    );
    assert_eq!(history.parameters.id(), buffer);
    assert_eq!(
        prepare(&mut history, true, true, 2),
        0.0,
        "another emitter rejects retained visibility"
    );
    assert_eq!(prepare(&mut history, true, true, 2), 1.0);
    assert_eq!(
        prepare(&mut history, true, false, 2),
        0.0,
        "camera cuts reject history"
    );
    assert_eq!(prepare(&mut history, true, true, 2), 1.0);
    assert_eq!(
        prepare(&mut history, false, true, 2),
        0.0,
        "unsubmitted frames never retain history"
    );
}
