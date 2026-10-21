//! Event-loop execution for the renderer's independently bounded background lanes.
use std::cell::RefCell;

type Job = Box<dyn FnOnce()>;

/// Each owner admits only one active job until its completion arrives.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Transparent,
    Models,
    Textures,
}

/// Fixed slots preserve each owner's admission bound without starting OS threads.
struct Pending {
    transparent: Option<Job>,
    models: Option<Job>,
    textures: Option<Job>,
    next: Lane,
}
thread_local! {
    static PENDING: RefCell<Pending> = const { RefCell::new(Pending { transparent: None, models: None, textures: None, next: Lane::Transparent }) };
}

impl Pending {
    /// Borrows the single slot belonging to the submitting renderer subsystem.
    fn slot(&mut self, lane: Lane) -> &mut Option<Job> {
        match lane {
            Lane::Transparent => &mut self.transparent,
            Lane::Models => &mut self.models,
            Lane::Textures => &mut self.textures,
        }
    }
}

/// Queues admitted work, replacing an unstarted job abandoned by an owner reset.
pub(crate) fn submit(lane: Lane, work: impl FnOnce() + 'static) {
    PENDING.with_borrow_mut(|pending| {
        let slot = pending.slot(lane);
        *slot = Some(Box::new(work));
    });
}

/// Drives at most one finite renderer job after the browser yields.
pub fn run_one() -> bool {
    let work = PENDING.with_borrow_mut(|pending| {
        let first = pending.next;
        loop {
            let lane = pending.next;
            pending.next = match lane {
                Lane::Transparent => Lane::Models,
                Lane::Models => Lane::Textures,
                Lane::Textures => Lane::Transparent,
            };
            if let Some(work) = pending.slot(lane).take() {
                return Some(work);
            }
            if pending.next == first {
                break;
            }
        }
        None
    });
    if let Some(work) = work {
        work();
        true
    } else {
        false
    }
}
