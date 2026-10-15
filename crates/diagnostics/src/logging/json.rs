//! JSON Lines records of log events for tooling, one object per event:
//! `{"timestamp_ms", "level", "thread", "target", "spans": [{"name", "fields"}], "message",
//! "fields"}`. `log`-crate records report their original target.
use crate::console::Priority;
use serde_json::{Map, Number, Value};
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

/// Receives each encoded record with its newline, at the event's priority.
pub(super) type Sink = fn(Priority, Vec<u8>);

pub(super) struct JsonLayer {
    sink: Sink,
}

impl JsonLayer {
    pub(super) fn new(sink: Sink) -> Self {
        Self { sink }
    }
}

/// A span's fields, kept in its extensions as they are recorded.
struct SpanFields(Map<String, Value>);

/// Collects an event's or span's fields as JSON values; `log.*` bookkeeping fields from
/// `log`-crate records are skipped.
#[derive(Default)]
struct Fields {
    message: Option<String>,
    values: Map<String, Value>,
}

impl Fields {
    fn insert(&mut self, field: &Field, value: Value) {
        let name = field.name();
        if name.starts_with("log.") {
            return;
        }
        if name == "message" {
            self.message = Some(match value {
                Value::String(text) => text,
                other => other.to_string(),
            });
        } else {
            self.values.insert(name.to_owned(), value);
        }
    }
}

impl Visit for Fields {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let value =
            Number::from_f64(value).map_or_else(|| Value::String(value.to_string()), Value::Number);
        self.insert(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, Value::Bool(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, Value::String(value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, Value::String(format!("{value:?}")));
    }
}

impl<S> Layer<S> for JsonLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(fields.values));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(SpanFields(stored)) = extensions.get_mut::<SpanFields>() {
            let mut fields = Fields {
                message: None,
                values: std::mem::take(stored),
            };
            values.record(&mut fields);
            *stored = fields.values;
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let normalized = event.normalized_metadata();
        let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let mut fields = Fields::default();
        event.record(&mut fields);
        let spans: Vec<Value> = ctx
            .event_scope(event)
            .into_iter()
            .flat_map(|scope| scope.from_root())
            .map(|span| {
                let extensions = span.extensions();
                let values = extensions
                    .get::<SpanFields>()
                    .map(|SpanFields(values)| values.clone())
                    .unwrap_or_default();
                serde_json::json!({ "name": span.name(), "fields": values })
            })
            .collect();
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let record = serde_json::json!({
            "timestamp_ms": timestamp_ms,
            "level": metadata.level().as_str(),
            "thread": std::thread::current().name(),
            "target": metadata.target(),
            "spans": spans,
            "message": fields.message,
            "fields": fields.values,
        });
        let mut line = record.to_string().into_bytes();
        line.push(b'\n');
        (self.sink)(Priority::of(metadata.level()), line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tracing_subscriber::layer::SubscriberExt;

    static RECORDS: Mutex<Vec<(Priority, Vec<u8>)>> = Mutex::new(Vec::new());

    fn collect(priority: Priority, line: Vec<u8>) {
        RECORDS.lock().unwrap().push((priority, line));
    }

    #[test]
    fn events_become_one_json_object_per_line_with_span_context() {
        let subscriber = tracing_subscriber::registry().with(JsonLayer::new(collect as Sink));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("join", server = "local");
            let _entered = span.enter();
            tracing::warn!(target: "client_session::join", chunks = 3, "slow join");
        });
        let records = std::mem::take(&mut *RECORDS.lock().unwrap());
        assert_eq!(records.len(), 1);
        let (priority, line) = &records[0];
        assert_eq!(*priority, Priority::Warn);
        assert_eq!(line.last(), Some(&b'\n'));
        let value: Value = serde_json::from_slice(line).unwrap();
        assert_eq!(value["level"], "WARN");
        assert_eq!(value["target"], "client_session::join");
        assert_eq!(value["message"], "slow join");
        assert_eq!(value["fields"]["chunks"], 3);
        assert_eq!(value["spans"][0]["name"], "join");
        assert_eq!(value["spans"][0]["fields"]["server"], "local");
        assert!(value["timestamp_ms"].as_u64().is_some());
    }
}
