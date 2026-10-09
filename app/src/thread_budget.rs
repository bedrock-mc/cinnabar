//! Process thread budget. World streaming sizes its own pool in chunk-pipeline; these
//! pools stay small so the frame and world workers are not oversubscribed.

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};

#[cfg(target_os = "macos")]
mod macos;

/// Thread counts for each shared pool on a machine with `cores` logical processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreadBudget {
    /// Transparent sorts and off-thread drops.
    pub(crate) rayon: usize,
    /// Frame systems; the main thread also runs them.
    pub(crate) compute: usize,
    /// Pipeline compilation.
    pub(crate) async_compute: usize,
    pub(crate) io: usize,
}

impl ThreadBudget {
    pub(crate) fn for_cores(cores: usize) -> Self {
        Self {
            rayon: (cores / 4).clamp(1, 2),
            compute: (cores / 2).clamp(2, 4),
            async_compute: if cores > 4 { 2 } else { 1 },
            io: 1,
        }
    }

    fn current() -> Self {
        Self::for_cores(std::thread::available_parallelism().map_or(1, usize::from))
    }

    pub(crate) fn task_pool_plugin() -> TaskPoolPlugin {
        let budget = Self::current();
        let fixed = |threads| TaskPoolThreadAssignmentPolicy {
            min_threads: threads,
            max_threads: threads,
            percent: 1.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        };
        TaskPoolPlugin {
            task_pool_options: TaskPoolOptions {
                io: fixed(budget.io),
                async_compute: fixed(budget.async_compute),
                compute: TaskPoolThreadAssignmentPolicy {
                    #[cfg(target_os = "macos")]
                    on_thread_spawn: Some(std::sync::Arc::new(macos::prepare_frame_thread)),
                    ..fixed(budget.compute)
                },
                ..TaskPoolOptions::default()
            },
        }
    }

    /// Configures frame submission on the render executor's own thread.
    #[cfg(target_os = "macos")]
    pub(crate) fn configure_render_thread(app: &mut bevy::prelude::App) {
        use bevy::{
            ecs::schedule::common_conditions::run_once,
            prelude::IntoScheduleConfigs,
            render::{Render, RenderApp, RenderSystems},
        };
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                Render,
                macos::prepare_render_thread
                    .run_if(run_once)
                    .before(RenderSystems::ExtractCommands),
            );
        }
    }

    /// Threads one join's pack compile may use: as many as world streaming runs, whose workers
    /// idle until that join's world exists, leaving the frame threads their cores.
    pub(crate) fn join_compile_threads(cores: usize) -> usize {
        chunk_pipeline::world_worker_threads(cores)
    }

    /// Runs `compile` on a pool sized by [`Self::join_compile_threads`] that exists only for
    /// this call, so its threads never compete with gameplay; on the caller's pool if the
    /// threads cannot start.
    pub(crate) fn on_join_compile_pool<T: Send>(compile: impl FnOnce() -> T + Send) -> T {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        match rayon::ThreadPoolBuilder::new()
            .num_threads(Self::join_compile_threads(cores))
            .thread_name(|index| format!("pack-compile-{index}"))
            .build()
        {
            Ok(pool) => pool.install(compile),
            Err(_) => compile(),
        }
    }

    /// Must run before anything touches the global rayon pool.
    pub(crate) fn configure_global_rayon() {
        let threads = Self::current().rayon;
        if let Err(error) = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("rayon-{index}"))
            .build_global()
        {
            eprintln!("global rayon pool was already configured: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared pools stay small at every gate tier instead of scaling with the core count.
    #[test]
    fn shared_pools_are_sized_for_4_8_and_12_cores() {
        let budget = |rayon, compute, async_compute| ThreadBudget {
            rayon,
            compute,
            async_compute,
            io: 1,
        };
        assert_eq!(ThreadBudget::for_cores(4), budget(1, 2, 1));
        assert_eq!(ThreadBudget::for_cores(8), budget(2, 4, 2));
        assert_eq!(ThreadBudget::for_cores(12), budget(2, 4, 2));
        assert_eq!(ThreadBudget::for_cores(1), budget(1, 2, 1));
    }

    /// A join compiles on as many threads as world streaming runs, more than the shared pool's.
    #[test]
    fn join_compiles_use_the_idle_world_cores() {
        for cores in [4, 8, 12] {
            assert!(
                ThreadBudget::join_compile_threads(cores) > ThreadBudget::for_cores(cores).rayon
            );
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        assert_eq!(
            ThreadBudget::on_join_compile_pool(rayon::current_num_threads),
            chunk_pipeline::world_worker_threads(cores)
        );
    }
}
