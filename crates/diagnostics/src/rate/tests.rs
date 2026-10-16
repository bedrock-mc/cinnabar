use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool},
    time::{Duration, Instant},
};

use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    span,
};

use super::{LogBudget, first};

const INTERVAL: Duration = Duration::from_secs(5);

fn at(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

#[test]
fn budget_admits_the_first_emission_and_counts_refusals_until_the_next() {
    let mut budget = LogBudget::<Duration>::new(INTERVAL);
    assert_eq!(budget.admit(at(100)), Some(0));
    assert_eq!(budget.admit(at(101)), None);
    assert_eq!(budget.admit(at(104)), None);
    assert_eq!(budget.suppressed(), 2);
    assert_eq!(budget.admit(at(105)), Some(2));
    assert_eq!(budget.suppressed(), 0);
    assert_eq!(budget.admit(at(109)), None);
    assert_eq!(budget.admit(at(200)), Some(1));
}

#[test]
fn budget_follows_only_the_time_its_caller_supplies() {
    let start = Instant::now();
    let mut budget = LogBudget::new(INTERVAL);
    assert!(budget.ready(start));
    assert_eq!(budget.admit(start), Some(0));
    for _ in 0..3 {
        assert!(
            !budget.ready(start),
            "the same instant never refills the budget"
        );
        assert_eq!(budget.admit(start), None);
    }
    assert!(
        !budget.ready(start + INTERVAL - Duration::from_nanos(1)),
        "one nanosecond short of the interval is refused"
    );
    assert_eq!(budget.admit(start + INTERVAL), Some(3));
    assert_eq!(
        budget.admit(start),
        None,
        "an earlier caller time counts as no time passed"
    );
}

#[test]
fn ready_records_nothing_and_reset_admits_the_next_emission_with_its_count() {
    let mut budget = LogBudget::<Duration>::new(INTERVAL);
    assert!(budget.ready(at(0)));
    assert!(
        budget.ready(at(0)),
        "checking readiness does not consume it"
    );
    assert_eq!(budget.admit(at(0)), Some(0));
    assert_eq!(budget.admit(at(1)), None);
    budget.reset();
    assert_eq!(budget.admit(at(2)), Some(1));
    assert_eq!(budget.admit(at(3)), None);
}

#[test]
fn first_is_true_once_per_flag() {
    let flag = AtomicBool::new(false);
    let other = AtomicBool::new(false);
    assert!(first(&flag));
    assert!(!first(&flag));
    assert!(!first(&flag));
    assert!(first(&other));
}

#[test]
fn log_every_reports_the_refused_count_on_the_next_event() {
    let events = capture(|| {
        let mut budget = LogBudget::<Duration>::new(INTERVAL);
        for second in [0, 1, 2, 3, 5, 6, 10] {
            crate::log_every!(budget, at(second), INFO, second, "drew unsorted");
        }
    });
    assert_eq!(
        events,
        [
            Captured::new(Level::INFO, "drew unsorted", Some(0)),
            Captured::new(Level::INFO, "drew unsorted", Some(3)),
            Captured::new(Level::INFO, "drew unsorted", Some(1)),
        ]
    );
}

#[test]
fn log_every_accepts_a_target_and_formatted_message() {
    let events = capture(|| {
        let mut budget = LogBudget::<Duration>::new(INTERVAL);
        let count = 7;
        crate::log_every!(budget, at(0), target: "render", WARN, "{count} chunks");
    });
    assert_eq!(events, [Captured::new(Level::WARN, "7 chunks", Some(0))]);
}

#[test]
fn warn_once_emits_once_per_callsite() {
    fn repeated() {
        crate::warn_once!("legacy setting ignored");
    }
    let events = capture(|| {
        for _ in 0..3 {
            repeated();
        }
        crate::warn_once!(target: "settings", "another site");
        crate::log_once!(ERROR, value = 1, "error site");
    });
    assert_eq!(
        events,
        [
            Captured::new(Level::WARN, "legacy setting ignored", None),
            Captured::new(Level::WARN, "another site", None),
            Captured::new(Level::ERROR, "error site", None),
        ]
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Captured {
    level: Level,
    message: String,
    suppressed: Option<u64>,
}

impl Captured {
    fn new(level: Level, message: &str, suppressed: Option<u64>) -> Self {
        Self {
            level,
            message: message.to_owned(),
            suppressed,
        }
    }
}

/// Runs `body` with a subscriber that records every event's level, message and `suppressed`.
fn capture(body: impl FnOnce()) -> Vec<Captured> {
    let events = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::with_default(Recorder(Arc::clone(&events)), body);
    std::mem::take(&mut *events.lock().unwrap_or_else(PoisonError::into_inner))
}

struct Recorder(Arc<Mutex<Vec<Captured>>>);

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut captured = Captured::new(*event.metadata().level(), "", None);
        event.record(&mut captured);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(captured);
    }

    fn enter(&self, _: &span::Id) {}

    fn exit(&self, _: &span::Id) {}
}

impl Visit for Captured {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "suppressed" {
            self.suppressed = Some(value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
}
