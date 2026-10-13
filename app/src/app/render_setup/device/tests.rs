use super::*;

#[test]
fn later_backend_named_adapter_wins_over_an_earlier_unnamed_gpu() {
    let mut calls = Vec::new();
    let result = search_backends(
        &[Backend::Vulkan, Backend::Dx12],
        Some("chosen"),
        |backend, name| {
            calls.push((backend, name.map(str::to_owned)));
            Ok(match (backend, name) {
                (Backend::Dx12, Some("chosen")) => Some("chosen GPU"),
                (_, None) => Some("other GPU"),
                _ => None,
            })
        },
    )
    .unwrap();
    assert_eq!(result, Some("chosen GPU"));
    assert_eq!(
        calls,
        [
            (Backend::Vulkan, Some("chosen".into())),
            (Backend::Dx12, Some("chosen".into()))
        ]
    );
}

#[test]
fn missing_name_falls_back_only_after_every_allowed_api_was_searched() {
    let mut calls = Vec::new();
    assert_eq!(
        search_backends(
            &[Backend::Vulkan, Backend::Gl],
            Some("missing"),
            |backend, name| {
                calls.push((backend, name.is_some()));
                Ok(name.is_none().then_some(backend))
            }
        )
        .unwrap(),
        Some(Backend::Vulkan)
    );
    assert_eq!(
        calls,
        [
            (Backend::Vulkan, true),
            (Backend::Gl, true),
            (Backend::Vulkan, false)
        ]
    );
    let mut calls = Vec::new();
    assert_eq!(
        search_backends(&[Backend::Dx12], None, |backend, name| {
            calls.push((backend, name.is_some()));
            Ok(Some(backend))
        })
        .unwrap(),
        Some(Backend::Dx12)
    );
    assert_eq!(calls, [(Backend::Dx12, false)]);
}

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
