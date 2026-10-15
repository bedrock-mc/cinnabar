//! The log flight recorder: the last [`crash_report::ring::flight::CAPACITY`] events its own
//! filter enables, formatted as one line each into a fixed ring that [`attach`] moves into the
//! crash staging directory, where a crash report reads it after any crash.
//!
//! Its filter is the base directives with [`CATEGORIES`] at `debug`, independent of the
//! session-file filter, so the recorder keeps detail the file leaves out. Only events that
//! filter enables are formatted, into a stack buffer: recording never allocates.

use std::{
    fmt::{self, Write as _},
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crash_report::{
    ring::flight::{self, FlightRing},
    staging,
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    EnvFilter,
    layer::{Context, Layer},
};

use super::Category;

/// Categories the recorder keeps at `debug`; every other target keeps the base filter's level.
pub const CATEGORIES: [Category; 4] = [
    Category::Net,
    Category::Session,
    Category::Core,
    Category::Packs,
];

/// The process's recorder ring.
static RING: FlightRing = FlightRing::new();

/// Moves the recorder ring, with the lines so far, into [`staging::FLIGHT_RING_FILE`] in the
/// crash staging directory `staging_dir`. Lines recorded during the move may be lost. Fails
/// when the file cannot be mapped or the recorder is already attached.
pub fn attach(staging_dir: &Path) -> io::Result<()> {
    RING.attach(&staging_dir.join(staging::FLIGHT_RING_FILE))
}

/// The recorder's filter: `base`, then each of [`CATEGORIES`] at `debug`.
pub(super) fn filter(base: &str) -> EnvFilter {
    let directives: Vec<String> = CATEGORIES
        .iter()
        .flat_map(|category| category.targets())
        .map(|target| format!("{target}=debug"))
        .collect();
    super::build_filter(base, &[], &directives)
}

/// Formats each event it sees into the ring as `LEVEL thread target: message fields`.
pub(super) struct FlightLayer;

impl<S: Subscriber> Layer<S> for FlightLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event
            .metadata()
            .fields()
            .field("tracy.frame_mark")
            .is_some()
        {
            return;
        }
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let mut line = Line::new();
        format_event(event, &mut line);
        RING.record(unix_ms, line.as_bytes());
    }
}

/// Writes `event` as the recorder's line; `log`-crate records keep their original target.
fn format_event(event: &Event<'_>, line: &mut Line) {
    let normalized = event.normalized_metadata();
    let metadata = normalized.as_ref().unwrap_or_else(|| event.metadata());
    let thread = std::thread::current();
    let _ = write!(
        line,
        "{:<5} {} {}:",
        metadata.level(),
        thread.name().unwrap_or("-"),
        metadata.target()
    );
    event.record(line);
}

/// One line in a stack buffer the size of a ring slot; writes past the end are dropped.
struct Line {
    bytes: [u8; flight::MAX_TEXT_BYTES],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            bytes: [0; flight::MAX_TEXT_BYTES],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl fmt::Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = self.bytes.len() - self.len;
        let taken = text.len().min(room);
        self.bytes[self.len..self.len + taken].copy_from_slice(&text.as_bytes()[..taken]);
        self.len += taken;
        Ok(())
    }
}

impl Visit for Line {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => {
                let _ = write!(self, " {value}");
            }
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(self, " {name}={value:?}");
            }
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => {
                let _ = write!(self, " {value:?}");
            }
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(self, " {name}={value:?}");
            }
        }
    }
}

/// The lines recorded so far, oldest first.
#[cfg(test)]
fn recorded_lines() -> Vec<flight::FlightLine> {
    RING.lines()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn events_format_to_one_bounded_line_without_allocating() {
        let subscriber = tracing_subscriber::registry().with(FlightLayer);
        tracing::subscriber::with_default(subscriber, || {
            let long = "y".repeat(flight::MAX_TEXT_BYTES);
            tracing::debug!(target: "client_session::join", chunks = 3, name = "spawn", "joined");
            let ((), allocations) = test_alloc::count(|| {
                tracing::warn!(target: "protocol::batch", id = 7_u32, "dropped {} bytes", 12);
            });
            assert_eq!(allocations, 0);
            tracing::info!(target: "packs", "{long}");
        });
        let lines = recorded_lines();
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        let thread = std::thread::current().name().unwrap_or("-").to_owned();
        let at = texts
            .iter()
            .position(|text| text.contains("joined"))
            .expect("the debug event");
        assert_eq!(
            texts[at],
            format!("DEBUG {thread} client_session::join: joined chunks=3 name=\"spawn\"")
        );
        assert_eq!(
            texts[at + 1],
            format!("WARN  {thread} protocol::batch: dropped 12 bytes id=7")
        );
        assert_eq!(texts[at + 2].len(), flight::MAX_TEXT_BYTES);
    }

    #[test]
    fn the_filter_keeps_recorder_categories_at_debug_over_the_base() {
        let filter = filter("info,wgpu_core=warn").to_string();
        for target in CATEGORIES.iter().flat_map(|category| category.targets()) {
            assert!(filter.contains(&format!("{target}=debug")), "{filter}");
        }
        assert!(filter.contains("wgpu_core=warn"), "{filter}");
    }
}
