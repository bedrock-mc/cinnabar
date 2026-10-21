use super::*;
use crate::pipeline_warmup::{PrewarmPipelines, WarmView};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::Msaa;

#[test]
fn loading_prepares_compositors_without_any_selections() {
    let (mut app, _) = crate::queue_review_support::app();
    app.world_mut().run_system_once(init).unwrap();
    let gpu = app.world().resource::<CompositionGpu>();
    assert!(gpu.blur.is_some());
    for format in [
        render_scene::SCENE_COLOR_FORMAT,
        render_scene::SCENE_HDR_FORMAT,
    ] {
        assert!(gpu.composite.contains_key(&format), "missing {format:?}");
    }
}

#[test]
fn loading_tracks_all_compositor_variants_and_reuses_them_for_other_views() {
    let (mut app, _) = crate::queue_review_support::app();
    let device = app.world().resource::<RenderDevice>().clone();
    app.insert_resource(PipelineCache::new(device, true));
    app.world_mut().run_system_once(init).unwrap();
    app.world_mut()
        .resource_scope(|world, mut gpu: bevy::prelude::Mut<CompositionGpu>| {
            let mut cache = world.resource_mut::<PipelineCache>();
            let view = WarmView {
                msaa: Msaa::Off,
                hdr: false,
                enhanced: false,
                output: None,
            };
            let mut ids = Vec::new();
            gpu.prewarm(&cache, view, &mut ids).unwrap();
            assert!(ids.contains(&gpu.blur.unwrap()));
            assert!(gpu.composite.values().all(|id| ids.contains(id)));
            assert_eq!(ids.len(), gpu.composite.len() + 1);
            assert!(
                ids.iter()
                    .all(|id| cache.get_render_pipeline(*id).is_none())
            );
            cache.process_queue();
            assert!(
                ids.iter()
                    .all(|id| cache.get_render_pipeline(*id).is_some())
            );
            let warmed = cache.pipelines().count();
            let mut next = Vec::new();
            gpu.prewarm(
                &cache,
                WarmView {
                    msaa: Msaa::Sample4,
                    hdr: true,
                    ..view
                },
                &mut next,
            )
            .unwrap();
            cache.process_queue();
            assert_eq!(next, ids);
            assert_eq!(cache.pipelines().count(), warmed);
        });
}

/// Allocates a sampled scene or coverage texture without uploading fixture data.
fn sampled_view(device: &RenderDevice) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some("composition binding fixture"),
            size: Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&default())
}

#[test]
fn unchanged_compositor_inputs_reuse_both_bindings_without_allocating() {
    let (app, _) = crate::queue_review_support::app();
    let device = app.world().resource::<RenderDevice>();
    let cache = app.world().resource::<PipelineCache>();
    let sampler = device.create_sampler(&default());
    let sources = [sampled_view(device), sampled_view(device)];
    let masks = [sampled_view(device), sampled_view(device)];
    let mut style = StyleGpu::new(device);
    style.prepare_bindings(
        device,
        cache,
        &sampler,
        sources.each_ref(),
        masks.each_ref(),
    );
    let warmed = sources
        .each_ref()
        .map(|source| style.binding(source.id()).unwrap().id());
    for _ in 0..4 {
        let (_, allocations) = test_alloc::count(|| {
            style.prepare_bindings(
                device,
                cache,
                &sampler,
                [&sources[1], &sources[0]],
                masks.each_ref(),
            );
            for (source, expected) in sources.iter().zip(warmed) {
                assert_eq!(style.binding(source.id()).unwrap().id(), expected);
            }
        });
        assert_eq!(allocations, 0);
    }
}

#[test]
fn compositor_bindings_replace_only_changed_resources_and_stay_bounded() {
    let (app, _) = crate::queue_review_support::app();
    let device = app.world().resource::<RenderDevice>();
    let cache = app.world().resource::<PipelineCache>();
    let sampler = device.create_sampler(&default());
    let mut sources = [sampled_view(device), sampled_view(device)];
    let mut masks = [sampled_view(device), sampled_view(device)];
    let mut style = StyleGpu::new(device);
    style.prepare_bindings(
        device,
        cache,
        &sampler,
        sources.each_ref(),
        masks.each_ref(),
    );
    let previous = sources
        .each_ref()
        .map(|source| style.binding(source.id()).unwrap().id());
    let removed = sources[0].id();
    sources[0] = sampled_view(device);
    style.prepare_bindings(
        device,
        cache,
        &sampler,
        sources.each_ref(),
        masks.each_ref(),
    );
    assert!(style.binding(removed).is_none());
    assert_ne!(style.binding(sources[0].id()).unwrap().id(), previous[0]);
    assert_eq!(style.binding(sources[1].id()).unwrap().id(), previous[1]);
    for index in [0, 1, 0, 1] {
        let previous = sources
            .each_ref()
            .map(|source| style.binding(source.id()).unwrap().id());
        masks[index] = sampled_view(device);
        style.prepare_bindings(
            device,
            cache,
            &sampler,
            sources.each_ref(),
            masks.each_ref(),
        );
        assert_eq!(style.bindings.len(), sources.len());
        for (source, old) in sources.iter().zip(previous) {
            assert_ne!(style.binding(source.id()).unwrap().id(), old);
        }
    }
    let mut other_style = StyleGpu::new(device);
    other_style.prepare_bindings(
        device,
        cache,
        &sampler,
        sources.each_ref(),
        masks.each_ref(),
    );
    for source in &sources {
        assert_ne!(
            style.binding(source.id()).unwrap().id(),
            other_style.binding(source.id()).unwrap().id()
        );
    }
    let replacement_sampler = device.create_sampler(&default());
    let previous = sources
        .each_ref()
        .map(|source| style.binding(source.id()).unwrap().id());
    style.prepare_bindings(
        device,
        cache,
        &replacement_sampler,
        sources.each_ref(),
        masks.each_ref(),
    );
    for (source, old) in sources.iter().zip(previous) {
        assert_ne!(style.binding(source.id()).unwrap().id(), old);
    }
    assert_eq!(style.bindings.len(), sources.len());
}
