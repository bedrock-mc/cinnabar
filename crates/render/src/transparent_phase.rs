//! Transient draws keep their authored distances and may share the same ECS entity.

use bevy::{
    core_pipeline::core_3d::Transparent3d,
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_phase::{SortedRenderPhase, ViewSortedRenderPhases, sort_phase_system},
        sync_world::MainEntity,
    },
};

#[derive(Resource)]
struct Installed;

/// Registers distance sorting once for renderers that queue already ordered transient draws.
pub(crate) fn install(app: &mut SubApp) {
    if app.world().contains_resource::<Installed>() {
        return;
    }
    app.insert_resource(Installed)
        .add_systems(RenderStartup, install_sorting);
}

/// Replaces retained-item distance recomputation with the final distances chosen by each owner.
fn install_sorting(world: &mut World) {
    let _ = world.try_schedule_scope(Render, |world, schedule| {
        let _ = schedule.remove_systems_in_set(
            sort_phase_system::<Transparent3d>,
            world,
            bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
        );
        schedule.add_systems(sort.in_set(RenderSystems::PhaseSort));
    });
}

/// Sorts by the queued distance, preserving insertion order for ties.
fn sort(mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>) {
    for phase in phases.values_mut() {
        phase.sort();
    }
}

/// Uses a transient map slot independent of the draw's real entities, so repeated draws survive.
pub(crate) fn add(phase: &mut SortedRenderPhase<Transparent3d>, item: Transparent3d) {
    let mut index =
        u32::try_from(phase.items.len()).expect("transparent phase exceeds entity index capacity");
    let key = loop {
        let slot =
            Entity::from_raw_u32(index).expect("transparent phase exceeds entity index capacity");
        // Bevy hashes the last entity in the pair, so that part must vary per draw.
        let key = (Entity::PLACEHOLDER, MainEntity::from(slot));
        if !phase.items.contains_key(&key) {
            break key;
        }
        index = index
            .checked_add(1)
            .expect("transparent phase exceeds entity index capacity");
    };
    phase.items.insert(key, item);
    phase.transient_items.push(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        core_pipeline::core_3d::TransparentSortingInfo3d,
        render::{
            render_phase::{AddRenderCommand, DrawFunctions, PhaseItemExtraIndex, SetItemPipeline},
            render_resource::CachedRenderPipelineId,
            view::RetainedViewEntity,
        },
    };

    #[test]
    fn repeated_draws_keep_entities_distances_ties_and_expire_next_frame() {
        let mut app = SubApp::new();
        app.init_resource::<DrawFunctions<Transparent3d>>()
            .add_render_command::<Transparent3d, SetItemPipeline>();
        let draw_function = app
            .world()
            .resource::<DrawFunctions<Transparent3d>>()
            .read()
            .id::<SetItemPipeline>();
        let entity = app.world_mut().spawn_empty().id();
        let main = MainEntity::from(entity);
        let view = RetainedViewEntity::new(main, None, 0);
        let mut phases = ViewSortedRenderPhases::<Transparent3d>::default();
        phases.prepare_for_new_frame(view);
        let phase = phases.get_mut(&view).unwrap();
        for (index, distance) in [7.0, -9.0, 7.0, f32::MAX].into_iter().enumerate() {
            add(
                phase,
                Transparent3d {
                    sorting_info: TransparentSortingInfo3d::AlwaysOnTop,
                    distance,
                    pipeline: CachedRenderPipelineId::INVALID,
                    entity: (entity, main),
                    draw_function,
                    batch_range: index as u32..index as u32 + 1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
        app.insert_resource(phases).add_systems(Render, sort);
        app.world_mut().run_schedule(Render);
        let phases = app
            .world()
            .resource::<ViewSortedRenderPhases<Transparent3d>>();
        let items = &phases[&view].items;
        assert_eq!(
            items
                .values()
                .map(|item| item.batch_range.start)
                .collect::<Vec<_>>(),
            [1, 0, 2, 3]
        );
        assert!(items.values().all(|item| item.entity == (entity, main)));
        assert_eq!(
            items.values().map(|item| item.distance).collect::<Vec<_>>(),
            [-9.0, 7.0, 7.0, f32::MAX]
        );
        let mut phases = app
            .world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
        phases.prepare_for_new_frame(view);
        assert!(phases[&view].items.is_empty());
    }
}
