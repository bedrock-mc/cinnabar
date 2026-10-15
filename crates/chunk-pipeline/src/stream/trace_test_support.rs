//! Captures fields at span entry, without relying on late record callbacks.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tracing::{Event, Metadata, Subscriber, field::Visit, span};

#[derive(Clone)]
pub(super) struct Observation {
    pub entered: bool,
    pub name: &'static str,
    pub fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct State {
    spans: BTreeMap<u64, (&'static str, BTreeMap<String, String>)>,
    observations: Vec<Observation>,
}

#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<State>>);

impl Capture {
    /// Runs a deterministic operation with a thread-local entry-only subscriber.
    pub fn with<T>(&self, work: impl FnOnce() -> T) -> T {
        tracing::subscriber::with_default(self.clone(), work)
    }

    /// Reads entered and exited spans in callback order.
    pub fn observations(&self) -> Vec<Observation> {
        self.0.lock().unwrap().observations.clone()
    }

    /// Captures one boundary using only fields supplied before entering it.
    fn observe(&self, id: &span::Id, entered: bool) {
        let mut state = self.0.lock().unwrap();
        let (name, fields) = state.spans[&id.into_u64()].clone();
        state.observations.push(Observation {
            entered,
            name,
            fields,
        });
    }
}

struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    /// Retains formatted values without depending on their concrete numeric type.
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl Subscriber for Capture {
    /// Enables the focused spans for this thread-local capture.
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    /// Stores fields available when the span is created.
    fn new_span(&self, attrs: &span::Attributes<'_>) -> span::Id {
        let mut fields = Fields(BTreeMap::new());
        attrs.record(&mut fields);
        let mut state = self.0.lock().unwrap();
        let id = state.spans.len() as u64 + 1;
        state.spans.insert(id, (attrs.metadata().name(), fields.0));
        span::Id::from_u64(id)
    }

    /// Deliberately ignores late fields, matching an export that emits text only on entry.
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    /// Relationships are irrelevant to the entry field contract.
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    /// Events are not used as evidence of entered span fields.
    fn event(&self, _: &Event<'_>) {}
    /// Captures the start boundary.
    fn enter(&self, id: &span::Id) {
        self.observe(id, true);
    }
    /// Captures the end boundary.
    fn exit(&self, id: &span::Id) {
        self.observe(id, false);
    }
}
