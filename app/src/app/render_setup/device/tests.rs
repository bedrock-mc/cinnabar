use super::*;

/// Supplies native-independent capabilities to the production candidate filter.
fn capabilities(flags: wgpu::DownlevelFlags, storage_buffers: u32) -> AdapterCapabilities {
    AdapterCapabilities {
        flags,
        limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: storage_buffers,
            ..Default::default()
        },
    }
}

#[test]
fn adapters_must_support_every_renderer_capability_and_storage_limit() {
    let required = render::required_vertex_storage_buffers();
    assert!(meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS, required)).is_ok());
    for flag in REQUIRED_DOWNLEVEL_FLAGS.iter() {
        assert!(
            meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS - flag, required)).is_err()
        );
    }
    assert!(meets_requirements(capabilities(REQUIRED_DOWNLEVEL_FLAGS, required - 1)).is_err());
}

#[test]
fn adapters_without_alternate_texture_views_are_not_candidates() {
    let view_formats = wgpu::DownlevelFlags::VIEW_FORMATS;
    assert!(REQUIRED_DOWNLEVEL_FLAGS.contains(view_formats));
    // Keep every other capability, isolating the missing GL/GLES texture-view support.
    let adapter = capabilities(
        wgpu::DownlevelFlags::all() - view_formats,
        render::required_vertex_storage_buffers(),
    );
    let error = meets_requirements(adapter).unwrap_err();
    assert!(error.to_string().contains("VIEW_FORMATS"));
}

#[test]
fn configured_storage_limits_cover_the_renderer_without_lowering_existing_limits() {
    let required = render::required_vertex_storage_buffers();
    for initial in [0, required, required + 1] {
        let mut settings = WgpuSettings::default();
        settings.limits.max_storage_buffers_per_shader_stage = initial;
        render::device_settings::configure(&mut settings);
        assert_eq!(
            settings.limits.max_storage_buffers_per_shader_stage,
            initial.max(required)
        );
    }
}
