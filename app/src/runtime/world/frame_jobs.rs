//! A browser has no world worker threads, so each frame runs queued world jobs on its own
//! thread, in the time its other work leaves before the display's next refresh.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use bevy::{
    app::{App, First},
    prelude::IntoScheduleConfigs,
    render::{Render, RenderApp, RenderSystems},
};
use chunk_pipeline::FrameJobBudget;
use web_time::Instant;

thread_local! {
    static BUDGET: RefCell<FrameJobBudget> = RefCell::default();
    /// When the current frame started and how long its world jobs ran.
    static FRAME: Cell<Option<(Instant, Duration)>> = const { Cell::new(None) };
}

/// Runs world jobs at the start of every frame and measures the frame's work once it has
/// rendered, which on a browser's single thread happens within the same update.
pub(crate) fn configure(app: &mut App) {
    app.add_systems(First, run_world_jobs);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(Render, finish_frame.after(RenderSystems::Cleanup));
    }
}

fn run_world_jobs() {
    let start = Instant::now();
    let budget = BUDGET.with_borrow_mut(|budget| budget.begin_frame(start));
    let jobs = chunk_pipeline::run_queued_world_jobs(budget);
    FRAME.set(Some((start, jobs)));
}

fn finish_frame() {
    if let Some((start, jobs)) = FRAME.take() {
        BUDGET.with_borrow_mut(|budget| budget.end_frame(start.elapsed(), jobs));
    }
}
