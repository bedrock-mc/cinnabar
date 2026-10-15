//! Log events in logcat, where an Android app's stderr goes nowhere.
use std::{ffi::CString, fmt::Write as _};
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

/// Writes each event to logcat, tagged with its target, as its message and `name=value` fields.
pub(super) struct AndroidLayer;

#[derive(Default)]
struct Text {
    message: String,
    fields: String,
}

impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        if name == "message" {
            let _ = write!(self.message, "{value:?}");
        } else if !name.starts_with("log.") {
            let _ = write!(self.fields, " {name}={value:?}");
        }
    }
}

/// Drops interior NULs, which logcat's C strings cannot carry.
fn c_string(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}

impl<S> Layer<S> for AndroidLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let normalized = event.normalized_metadata();
        let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let mut text = Text::default();
        event.record(&mut text);
        let priority = match *metadata.level() {
            Level::TRACE => android_log_sys::LogPriority::VERBOSE,
            Level::DEBUG => android_log_sys::LogPriority::DEBUG,
            Level::INFO => android_log_sys::LogPriority::INFO,
            Level::WARN => android_log_sys::LogPriority::WARN,
            Level::ERROR => android_log_sys::LogPriority::ERROR,
        };
        let tag = c_string(metadata.target());
        let message = c_string(&(text.message + &text.fields));
        // SAFETY: Both strings are NUL-terminated and outlive the call; the priority is a
        // valid logcat priority.
        unsafe {
            android_log_sys::__android_log_write(
                priority as android_log_sys::c_int,
                tag.as_ptr(),
                message.as_ptr(),
            );
        }
    }
}
