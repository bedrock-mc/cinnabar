//! Executor choice for the schedules of the main and render worlds.

use bevy::{
    app::{App, SubApp},
    ecs::schedule::{ExecutorKind, Schedules},
    render::RenderApp,
};

/// Runs every schedule of the main and render worlds on one thread each. At several hundred
/// frames per second the parallel executor's per-system task wake-ups cost more than its
/// parallelism returns, while meshing, lighting and skin preparation already run on their own
/// task pools. Call once every plugin has added its schedules.
pub(super) fn run_frame_schedules_on_one_thread(app: &mut App) {
    run_schedules_on_one_thread(app.main_mut());
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        run_schedules_on_one_thread(render_app);
    }
}

fn run_schedules_on_one_thread(app: &mut SubApp) {
    let Some(mut schedules) = app.world_mut().get_resource_mut::<Schedules>() else {
        return;
    };
    for (_, schedule) in schedules.iter_mut() {
        schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        app::{Last, Main, Update},
        ecs::schedule::{Schedule, ScheduleLabel},
        render::{ExtractSchedule, Render},
    };

    use super::*;

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    struct Custom;

    #[test]
    fn every_schedule_of_both_worlds_runs_on_one_thread() {
        let mut app = App::new();
        app.add_systems(Update, || {})
            .add_systems(Last, || {})
            .init_schedule(Custom);
        let mut render_app = SubApp::new();
        render_app
            .add_schedule(Schedule::new(Render))
            .add_schedule(Schedule::new(ExtractSchedule));
        app.insert_sub_app(RenderApp, render_app);
        run_frame_schedules_on_one_thread(&mut app);
        for label in [
            Main.intern(),
            Update.intern(),
            Last.intern(),
            Custom.intern(),
        ] {
            let schedule = app.get_schedule(label).expect("main schedule exists");
            assert_eq!(schedule.get_executor_kind(), ExecutorKind::SingleThreaded);
        }
        let render_app = app.sub_app(RenderApp);
        for label in [Render.intern(), ExtractSchedule.intern()] {
            let schedule = render_app
                .get_schedule(label)
                .expect("render schedule exists");
            assert_eq!(schedule.get_executor_kind(), ExecutorKind::SingleThreaded);
        }
    }
}
