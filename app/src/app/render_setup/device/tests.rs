use super::*;

#[test]
fn adapters_must_support_terrain_base_vertex_and_storage() {
    let storage = wgpu::DownlevelFlags::VERTEX_STORAGE;
    let base_vertex = wgpu::DownlevelFlags::BASE_VERTEX;
    let required = render::required_vertex_storage_buffers();
    assert!(validate_capabilities(storage, required).is_err());
    assert!(validate_capabilities(base_vertex, required).is_err());
    assert!(validate_capabilities(storage | base_vertex, required - 1).is_err());
    assert!(validate_capabilities(storage | base_vertex, required).is_ok());
}

#[test]
fn configured_storage_limits_cover_the_renderer_without_lowering_existing_limits() {
    let required = render::required_vertex_storage_buffers();
    for initial in [0, required, required + 1] {
        let mut settings = WgpuSettings::default();
        settings.limits.max_storage_buffers_per_shader_stage = initial;
        configure_render_settings(&mut settings);
        assert_eq!(
            settings.limits.max_storage_buffers_per_shader_stage,
            initial.max(required)
        );
    }
}
