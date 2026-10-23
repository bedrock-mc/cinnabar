//! Prepares world GPU owners off the render thread after menu completion.

use bevy::{
    prelude::*,
    render::{Render, RenderSystems},
};
use render_scene::startup_work::{WorldWorkAdmission, world_work_allowed};

#[derive(Resource)]
struct Pending<T: Resource> {
    create: Option<Box<dyn FnOnce() -> T + Send + Sync>>,
    worker: Option<std::thread::JoinHandle<T>>,
}

/// Keeps direct joins eager and moves menu launches' expensive GPU constructors to a worker.
pub(crate) fn install<T: Resource>(
    app: &mut SubApp,
    create: impl FnOnce() -> T + Send + Sync + 'static,
) {
    if app
        .world()
        .get_resource::<WorldWorkAdmission>()
        .is_none_or(WorldWorkAdmission::is_admitted)
    {
        app.insert_resource(create());
    } else {
        crate::pipeline_warmup::register_pending_resource::<Pending<T>>(app);
        app.insert_resource(Pending::<T> {
            create: Some(Box::new(create)),
            worker: None,
        })
        .add_systems(
            Render,
            poll::<T>
                .before(RenderSystems::PrepareResources)
                .run_if(world_work_allowed),
        );
    }
}

/// Publishes a completed owner before its draw preparation, without blocking a menu frame.
fn poll<T: Resource>(world: &mut World) {
    let Some(mut pending) = world.remove_resource::<Pending<T>>() else {
        return;
    };
    if let Some(create) = pending.create.take() {
        pending.worker = Some(std::thread::spawn(create));
    }
    if pending
        .worker
        .as_ref()
        .is_some_and(std::thread::JoinHandle::is_finished)
    {
        world.insert_resource(
            pending
                .worker
                .take()
                .expect("finished GPU preparation")
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        );
    } else {
        world.insert_resource(pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Resource)]
    struct Prepared;

    #[test]
    fn world_gpu_construction_waits_for_menu_admission() {
        let mut app = SubApp::new();
        app.init_schedule(Render);
        let admission = WorldWorkAdmission::after_menu();
        app.insert_resource(admission.clone());
        let count = Arc::new(AtomicUsize::new(0));
        let worker = count.clone();
        install(&mut app, move || {
            worker.fetch_add(1, Ordering::Relaxed);
            Prepared
        });
        app.world_mut().run_schedule(Render);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(!app.world().contains_resource::<Prepared>());
        admission.admit();
        test_time::eventually("world GPU preparation published", || {
            app.world_mut().run_schedule(Render);
            app.world().contains_resource::<Prepared>()
        });
        assert_eq!(count.load(Ordering::Relaxed), 1);
        app.world_mut().run_schedule(Render);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
