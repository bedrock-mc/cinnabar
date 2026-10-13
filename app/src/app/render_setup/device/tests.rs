use super::*;

#[test]
fn later_backend_named_adapter_wins_over_an_earlier_unnamed_gpu() {
    let mut calls = Vec::new();
    let result = search_backends(
        &[Backend::Vulkan, Backend::Dx12],
        Some("chosen"),
        |backends, name| {
            calls.push((backends.to_vec(), name.map(str::to_owned)));
            Ok(match (backends, name) {
                ([Backend::Dx12], Some("chosen")) => Some("chosen GPU"),
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
            (vec![Backend::Vulkan], Some("chosen".into())),
            (vec![Backend::Dx12], Some("chosen".into()))
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
            |backends, name| {
                calls.push((backends.to_vec(), name.is_some()));
                Ok(name.is_none().then(|| backends.to_vec()))
            }
        )
        .unwrap(),
        Some(vec![Backend::Vulkan, Backend::Gl])
    );
    assert_eq!(
        calls,
        [
            (vec![Backend::Vulkan], true),
            (vec![Backend::Gl], true),
            (vec![Backend::Vulkan, Backend::Gl], false)
        ]
    );
    let mut calls = Vec::new();
    assert_eq!(
        search_backends(&[Backend::Dx12], None, |backends, name| {
            calls.push((backends.to_vec(), name.is_some()));
            Ok(Some(backends.to_vec()))
        })
        .unwrap(),
        Some(vec![Backend::Dx12])
    );
    assert_eq!(calls, [(vec![Backend::Dx12], false)]);
}

#[test]
fn unnamed_selection_ranks_discrete_dx12_above_integrated_or_software_vulkan() {
    let mut settings = WgpuSettings::default();
    configure_render_settings(&mut settings);
    assert_eq!(
        settings.power_preference,
        wgpu::PowerPreference::HighPerformance
    );

    for vulkan_type in [wgpu::DeviceType::IntegratedGpu, wgpu::DeviceType::Cpu] {
        let hardware = [
            (Backend::Vulkan, vulkan_type),
            (Backend::Dx12, wgpu::DeviceType::DiscreteGpu),
        ];
        let mut calls = Vec::new();
        let selected =
            search_backends(&[Backend::Vulkan, Backend::Dx12], None, |backends, name| {
                calls.push((backends.to_vec(), name.is_some()));
                // Model a high-performance request using only the APIs visible to this instance.
                let mut visible = hardware
                    .iter()
                    .copied()
                    .filter(|(api, _)| backends.contains(api));
                Ok(visible
                    .clone()
                    .find(|(_, kind)| *kind == wgpu::DeviceType::DiscreteGpu)
                    .or_else(|| visible.next()))
            })
            .unwrap();
        assert_eq!(
            selected,
            Some((Backend::Dx12, wgpu::DeviceType::DiscreteGpu))
        );
        assert_eq!(calls, [(vec![Backend::Vulkan, Backend::Dx12], false)]);
    }
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
