//! Browser frame driver over the existing world queues. Native hosts retain their worker pool.

use simulation_clock::Moment;

/// Scratch retained by the browser's single world-job driver.
#[derive(Default)]
pub struct WorldJobDriver {
    scratch: world::LightSolverScratch,
}

impl WorldJobDriver {
    /// Supplies a monotonic observation before the frame submits work.
    pub fn observe(&mut self, now: Moment) {
        crate::stream::workers::observe(now);
    }

    /// Runs one finite job after yielding, preserving native lane order and cancellation.
    /// The browser host must measure indivisible job cost before admitting this driver to play.
    pub fn run_one(&mut self, now: Moment) -> bool {
        crate::stream::workers::run_browser_job(now, &mut self.scratch)
    }
}
