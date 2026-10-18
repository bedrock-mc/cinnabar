//! Bounded browser execution for UI preparation after yielding to the event loop.
use std::cell::RefCell;
type Job = Box<dyn FnMut() -> bool>;
/// Independently superseded work from the retained UI's three preparation owners.
#[derive(Clone, Copy)]
pub(crate) enum Lane {
    Icons,
    Model,
    Artwork,
}
#[derive(Default)]
struct Pending {
    icons: Option<Job>,
    model: Option<Job>,
    artwork: Option<Job>,
}
thread_local! { static PENDING: RefCell<Pending> = RefCell::default(); }
impl Pending {
    /// Selects the bounded slot belonging to one preparation owner.
    fn slot(&mut self, lane: Lane) -> &mut Option<Job> {
        match lane {
            Lane::Icons => &mut self.icons,
            Lane::Model => &mut self.model,
            Lane::Artwork => &mut self.artwork,
        }
    }
}
/// Supersedes unstarted work when its retained UI owner changes the requested source.
pub(crate) fn submit(lane: Lane, work: impl FnOnce() + 'static) {
    let mut work = Some(work);
    submit_steps(lane, move || {
        work.take().expect("one-shot UI work")();
        true
    });
}
/// Keeps one resumable preparation, whose return value reports completion.
pub(crate) fn submit_steps(lane: Lane, work: impl FnMut() -> bool + 'static) {
    PENDING.with_borrow_mut(|pending| *pending.slot(lane) = Some(Box::new(work)));
}
/// Runs a finite preparation step, alternating independently admitted work.
#[derive(Default)]
pub struct Driver {
    next: usize,
}
impl Driver {
    /// Performs at most one step; the browser host must yield before calling again.
    pub fn run_one(&mut self) -> bool {
        let lanes = [Lane::Icons, Lane::Model, Lane::Artwork];
        for _ in lanes {
            let lane = lanes[self.next];
            self.next = (self.next + 1) % lanes.len();
            let work = PENDING.with_borrow_mut(|pending| pending.slot(lane).take());
            if let Some(mut work) = work {
                if !work() {
                    PENDING.with_borrow_mut(|pending| {
                        pending.slot(lane).get_or_insert(work);
                    });
                }
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn superseded_work_never_publishes_and_steps_yield_between_lanes() {
        let output = Rc::new(RefCell::new(Vec::new()));
        let old = output.clone();
        submit(Lane::Icons, move || old.borrow_mut().push("old"));
        let fresh = output.clone();
        submit(Lane::Icons, move || fresh.borrow_mut().push("icons"));
        let model = output.clone();
        submit(Lane::Model, move || model.borrow_mut().push("model"));
        let artwork = output.clone();
        let mut steps = 0;
        submit_steps(Lane::Artwork, move || {
            artwork.borrow_mut().push("artwork");
            steps += 1;
            steps == 2
        });
        let mut driver = Driver::default();
        assert!(output.borrow().is_empty());
        for expected in 1..=4 {
            assert!(driver.run_one());
            assert_eq!(output.borrow().len(), expected);
        }
        assert!(!driver.run_one());
        assert_eq!(&*output.borrow(), &["icons", "model", "artwork", "artwork"]);
    }
}
