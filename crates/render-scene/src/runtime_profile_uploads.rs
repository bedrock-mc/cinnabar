//! Labelled upload requests. Counts describe writes per path; a partially staged request
//! contributes once to each path, while its bytes are split without duplication.

/// Resource receiving an upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UploadKind {
    Buffer,
    Texture,
}

/// Whether an upload used retained mapping or queue-allocated staging.
/// Queue writes include direct uploads and retained-pool fallbacks.
#[derive(Debug, Clone, Copy)]
pub enum UploadPath {
    Staged,
    Queue,
}

#[cfg(feature = "publication-test-support")]
thread_local! {
    static TEST_WRITES: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// Returns `work`'s result and its nonempty profiled (write count, bytes) on this thread.
/// Nested measurements each include their own work without resetting shared diagnostics.
#[cfg(feature = "publication-test-support")]
pub fn measure<T>(work: impl FnOnce() -> T) -> (T, (u64, u64)) {
    let before = TEST_WRITES.get();
    let result = work();
    let after = TEST_WRITES.get();
    (
        result,
        (
            after.0.wrapping_sub(before.0),
            after.1.wrapping_sub(before.1),
        ),
    )
}

/// Records a nonempty upload at its original call site when diagnostics are compiled in.
#[inline]
#[track_caller]
pub fn record(kind: UploadKind, path: UploadPath, bytes: u64) {
    #[cfg(any(
        test,
        feature = "alloc-stats",
        feature = "tracy",
        feature = "publication-test-support"
    ))]
    record_at(std::panic::Location::caller(), kind, path, bytes);
    #[cfg(not(any(
        test,
        feature = "alloc-stats",
        feature = "tracy",
        feature = "publication-test-support"
    )))]
    let _ = (kind, path, bytes);
}

/// Records an upload whose helper retained the original source location.
#[inline]
pub fn record_at(
    site: &'static std::panic::Location<'static>,
    kind: UploadKind,
    path: UploadPath,
    bytes: u64,
) {
    #[cfg(feature = "publication-test-support")]
    if bytes != 0 {
        TEST_WRITES.update(|(calls, total)| (calls.wrapping_add(1), total.wrapping_add(bytes)));
    }
    #[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
    enabled::record(site, kind, path, bytes);
    #[cfg(not(any(test, feature = "alloc-stats", feature = "tracy")))]
    let _ = (site, kind, path, bytes);
}

/// Closes the preceding upload interval and emits Tracy plots when available.
pub(super) fn finish_frame() {
    #[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
    enabled::finish_frame();
}

/// Copies the completed interval for deferred formatting when a slow-frame line is emitted.
pub(super) fn snapshot() -> UploadFrame {
    #[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
    {
        enabled::snapshot()
    }
    #[cfg(not(any(test, feature = "alloc-stats", feature = "tracy")))]
    UploadFrame {}
}

/// Owned upload samples that remain valid after later frame boundaries.
pub(super) struct UploadFrame {
    #[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
    sites: Vec<(enabled::Key, enabled::Counts)>,
}

impl UploadFrame {
    /// Formats labelled staged and queue counts without locking the live table.
    pub(super) fn append_fields(&self, line: &mut String) {
        #[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
        {
            use std::fmt::Write;
            line.push_str(" gpu_writes=");
            let mut separator = "";
            for ((file, row, column, kind), counts) in &self.sites {
                let _ = write!(
                    line,
                    "{separator}{file}:{row}:{column}/{kind:?}:staged={}:{}:queue={}:{}",
                    counts.staged_writes,
                    counts.staged_bytes,
                    counts.queue_writes,
                    counts.queue_bytes
                );
                separator = ",";
            }
        }
        #[cfg(not(any(test, feature = "alloc-stats", feature = "tracy")))]
        let _ = line;
    }
}

#[cfg(any(test, feature = "alloc-stats", feature = "tracy"))]
mod enabled {
    use super::{UploadFrame, UploadKind, UploadPath};
    use std::{collections::BTreeMap, panic::Location, sync::Mutex};

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Counts {
        pub(super) staged_writes: u64,
        pub(super) staged_bytes: u64,
        pub(super) queue_writes: u64,
        pub(super) queue_bytes: u64,
    }

    impl Counts {
        /// Lists every stored metric; adding a counter requires updating this pattern.
        #[cfg(feature = "tracy")]
        fn metrics(self) -> [(&'static str, u64); 4] {
            let Self {
                staged_writes,
                staged_bytes,
                queue_writes,
                queue_bytes,
            } = self;
            [
                ("staged_writes", staged_writes),
                ("staged_bytes", staged_bytes),
                ("queue_writes", queue_writes),
                ("queue_bytes", queue_bytes),
            ]
        }

        /// Adds one nonempty write to its actual path.
        fn record(&mut self, path: UploadPath, bytes: u64) {
            if bytes == 0 {
                return;
            }
            match path {
                UploadPath::Staged => {
                    self.staged_writes += 1;
                    self.staged_bytes += bytes;
                }
                UploadPath::Queue => {
                    self.queue_writes += 1;
                    self.queue_bytes += bytes;
                }
            }
        }
    }

    #[derive(Default)]
    struct Site {
        current: Counts,
        last: Counts,
        #[cfg(feature = "tracy")]
        plots: Option<[tracy_client::PlotName; 4]>,
    }

    pub(super) type Key = (&'static str, u32, u32, UploadKind);
    struct UploadCounters(BTreeMap<Key, Site>);
    static SITES: Mutex<UploadCounters> = Mutex::new(UploadCounters(BTreeMap::new()));

    /// Records into the shared caller table.
    pub(super) fn record(
        site: &'static Location<'static>,
        kind: UploadKind,
        path: UploadPath,
        bytes: u64,
    ) {
        SITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(site, kind, path, bytes);
    }

    /// Publishes one upload interval from the shared table.
    pub(super) fn finish_frame() {
        SITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish_frame();
    }

    /// Copies active samples from the shared table's most recently completed interval.
    pub(super) fn snapshot() -> UploadFrame {
        SITES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    impl UploadCounters {
        /// Keeps one counter entry per source location, independent of resource lifetimes.
        fn record(
            &mut self,
            site: &'static Location<'static>,
            kind: UploadKind,
            path: UploadPath,
            bytes: u64,
        ) {
            if bytes == 0 {
                return;
            }
            self.0
                .entry((site.file(), site.line(), site.column(), kind))
                .or_default()
                .current
                .record(path, bytes);
        }

        /// Drains counts without discarding site identities or allocating on subsequent frames.
        fn finish_frame(&mut self) {
            for site in self.0.values_mut() {
                site.last = std::mem::take(&mut site.current);
            }
            #[cfg(feature = "tracy")]
            if let Some(client) = tracy_client::Client::running() {
                for ((file, line, column, kind), site) in &mut self.0 {
                    let plots = site.plots.get_or_insert_with(|| {
                        site.last.metrics().map(|(metric, _)| {
                            tracy_client::PlotName::new_leak(format!(
                                "upload/{file}:{line}:{column}/{kind:?}/{metric}"
                            ))
                        })
                    });
                    for (name, (_, value)) in plots.iter().zip(site.last.metrics()) {
                        client.plot(*name, value as f64);
                    }
                }
            }
        }

        /// Retains only active sites; this is requested at most once per slow-frame log interval.
        fn snapshot(&self) -> UploadFrame {
            UploadFrame {
                sites: self
                    .0
                    .iter()
                    .filter(|(_, site)| site.last != Counts::default())
                    .map(|(key, site)| (*key, site.last))
                    .collect(),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Preserves caller and resource labels, splits partial writes and drains each frame.
        #[test]
        fn labelled_intervals_split_paths_and_clear_idle_sites() {
            let mut counters = UploadCounters(BTreeMap::new());
            let first = Location::caller();
            let second = Location::caller();
            assert_ne!(first, second);
            counters.record(first, UploadKind::Buffer, UploadPath::Staged, 64);
            counters.record(first, UploadKind::Buffer, UploadPath::Queue, 32);
            counters.record(first, UploadKind::Texture, UploadPath::Queue, 128);
            counters.record(second, UploadKind::Buffer, UploadPath::Queue, 16);
            counters.record(second, UploadKind::Texture, UploadPath::Queue, 0);
            counters.finish_frame();
            let snapshot = counters.snapshot();
            // Formatting may be delayed past a later frame's drain.
            counters.finish_frame();
            let mut fields = String::new();
            snapshot.append_fields(&mut fields);
            assert!(
                fields.contains(&format!("{first}/Buffer:staged=1:64:queue=1:32")),
                "{fields}"
            );
            assert!(
                fields.contains(&format!("{first}/Texture:staged=0:0:queue=1:128")),
                "{fields}"
            );
            assert!(
                fields.contains(&format!("{second}/Buffer:staged=0:0:queue=1:16")),
                "{fields}"
            );
            assert_eq!(counters.0.len(), 3);
            counters.finish_frame();
            fields.clear();
            counters.snapshot().append_fields(&mut fields);
            assert_eq!(fields, " gpu_writes=");
            assert_eq!(counters.0.len(), 3);
            let (_, overhead) = test_alloc::measure(|| {
                counters.record(first, UploadKind::Buffer, UploadPath::Queue, 4);
                counters.finish_frame();
            });
            assert_eq!(overhead.calls, 0);
        }
    }
}
