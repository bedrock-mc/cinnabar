//! Actual slot visits during owner-selection preparation and mutation restoration.

/// Slot work counted at the operations that reset, inherit, assign, and restore values.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Work {
    pub reset: usize,
    pub inherited: usize,
    pub assigned: usize,
    pub saved: usize,
    pub restored: usize,
}

thread_local! {
    static WORK: std::cell::Cell<Work> = const { std::cell::Cell::new(Work {
        reset: 0, inherited: 0, assigned: 0, saved: 0, restored: 0,
    }) };
}

/// Counts slot visits at an existing fill, loop, assignment, or journal restoration.
pub(super) fn record(update: impl FnOnce(&mut Work)) {
    WORK.with(|work| {
        let mut value = work.get();
        update(&mut value);
        work.set(value);
    });
}

/// Takes only the slot work performed on this test thread since its previous observation.
pub(crate) fn take() -> Work {
    WORK.with(|work| work.replace(Work::default()))
}
