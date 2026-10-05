use crate::chunk::*;

pub(in crate::chunk) fn install(render_app: &mut SubApp) {
    render_app.add_systems(
        Render,
        (
            prepare_view_pipelines.in_set(RenderSystems::Queue),
            prepare_queued_pipelines
                .after(RenderSystems::PrepareBindGroups)
                .before(RenderSystems::Render),
        ),
    );
}

fn prepare_view_pipelines(
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<ChunkPipeline>,
    views: Query<(&ExtractedView, &Msaa, Option<&crate::EnhancedRendering>), With<Camera3d>>,
) {
    let pipelines = &mut *pipelines;
    for (view, msaa, enhanced) in &views {
        let key = ChunkPipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
            enhanced: enhanced.is_some(),
        };
        for variants in [
            &mut pipelines.variants,
            &mut pipelines.model_variants,
            &mut pipelines.transparent_model_variants,
            &mut pipelines.liquid_variants,
            &mut pipelines.depth_liquid_variants,
        ] {
            let _ = variants.specialize(&cache, key);
        }
    }
}

fn prepare_queued_pipelines(
    mut cache: ResMut<PipelineCache>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::PipelinePreparation));
    cache.process_queue();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_world_prepares_actual_view_pipelines_once_before_terrain_arrives() {
        let (mut app, _) = crate::queue_review_support::app();
        app.init_resource::<ChunkPipeline>()
            .init_resource::<crate::PanoramaScene>();
        app.world_mut()
            .resource_mut::<crate::PanoramaScene>()
            .set_game_visible(false);
        let world = app.world_mut();
        let mut views = world.query_filtered::<Entity, With<ExtractedView>>();
        let view = views.single(world).unwrap();
        world.entity_mut(view).insert(Camera3d::default());
        install(app.main_mut());
        assert_eq!(pipeline_count(&mut app), 0);
        app.world_mut().run_schedule(Render);
        assert_eq!(pipeline_count(&mut app), 5);
        app.world_mut().run_schedule(Render);
        assert_eq!(pipeline_count(&mut app), 5);

        let world = app.world_mut();
        let mut views = world.query_filtered::<&mut ExtractedView, With<Camera3d>>();
        views.single_mut(world).unwrap().hdr = true;
        app.world_mut().run_schedule(Render);
        assert_eq!(pipeline_count(&mut app), 10);
        app.world_mut()
            .resource_mut::<crate::PanoramaScene>()
            .set_game_visible(true);
        app.world_mut().run_schedule(Render);
        assert_eq!(pipeline_count(&mut app), 10);
    }

    fn pipeline_count(app: &mut App) -> usize {
        let mut cache = app.world_mut().resource_mut::<PipelineCache>();
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        cache.process_queue();
        cache.pipelines().count()
    }
}
