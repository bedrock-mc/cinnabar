/// Frame systems need the same scheduling class as native UI and animation work.
pub(super) fn prepare_frame_thread() {
    // SAFETY: This changes only the calling frame worker's requested QoS.
    let status = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    if status != 0 {
        bevy::log::warn!(
            status,
            "could not set interactive scheduling for a frame thread"
        );
    }
}

/// Exclusive execution keeps this one-time policy on the render schedule's own thread.
pub(super) fn prepare_render_thread(_world: &mut bevy::prelude::World) {
    prepare_frame_thread();
}

#[cfg(test)]
mod tests {
    use crate::thread_budget::ThreadBudget;
    use bevy::{
        app::SubApp,
        prelude::App,
        render::{Render, RenderApp},
    };

    /// Establishes a lower starting class on this isolated test thread only.
    fn start_at_default_qos() {
        // SAFETY: The API changes only the calling test thread.
        let status =
            unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_DEFAULT, 0) };
        assert_eq!(status, 0);
    }

    /// Reads the requested native class without relying on scheduling or elapsed time.
    fn assert_qos(expected: libc::qos_class_t) {
        let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        let mut relative = 0;
        // SAFETY: Both outputs are valid and pthread_self identifies this live thread.
        let status = unsafe {
            libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
        };
        assert_eq!(status, 0);
        assert_eq!(class as u32, expected as u32);
    }

    #[test]
    fn compute_frame_worker_requests_interactive_qos() {
        let policy = ThreadBudget::task_pool_plugin().task_pool_options.compute;
        std::thread::spawn(move || {
            start_at_default_qos();
            if let Some(started) = policy.on_thread_spawn {
                started();
            }
            assert_qos(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn background_worker_policies_keep_the_default_class() {
        let policies = ThreadBudget::task_pool_plugin().task_pool_options;
        for policy in [policies.io, policies.async_compute] {
            std::thread::spawn(move || {
                start_at_default_qos();
                if let Some(started) = policy.on_thread_spawn {
                    started();
                }
                assert_qos(libc::qos_class_t::QOS_CLASS_DEFAULT);
            })
            .join()
            .unwrap();
        }
    }

    #[test]
    fn render_executor_requests_interactive_qos_on_its_own_thread() {
        std::thread::spawn(|| {
            start_at_default_qos();
            let mut app = App::new();
            let mut render = SubApp::new();
            render.init_schedule(Render);
            app.insert_sub_app(RenderApp, render);
            ThreadBudget::configure_render_thread(&mut app);
            assert_qos(libc::qos_class_t::QOS_CLASS_DEFAULT);
            app.sub_app_mut(RenderApp).world_mut().run_schedule(Render);
            assert_qos(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE);
            start_at_default_qos();
            app.sub_app_mut(RenderApp).world_mut().run_schedule(Render);
            assert_qos(libc::qos_class_t::QOS_CLASS_DEFAULT);
        })
        .join()
        .unwrap();
    }
}
