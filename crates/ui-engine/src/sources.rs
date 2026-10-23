//! Host-owned reads, clock and threads the texture atlas draws through, keeping filesystem,
//! network, time and thread access out of the engine.

use render_api::BodyGeneration;
use std::{sync::Arc, time::Duration};

/// The installed vanilla resource pack's files by pack-relative path; the host owns the reads.
pub trait LocalPack: Send + Sync {
    /// The size of the file at `relative`, spelled exactly so; `None` when absent or outside
    /// the pack.
    fn file_size(&self, relative: &str) -> Option<u64>;

    /// The file's bytes when it holds at most `limit`; `None` when absent, unreadable or larger.
    fn read(&self, relative: &str, limit: u64) -> Option<Vec<u8>>;
}

/// Images named by URL; the host owns the downloads and their cache.
pub trait RemoteSource: Send + Sync {
    /// Where `url`'s download stands, starting it on first sight; asking marks it in use.
    fn state(&self, url: &str) -> RemoteImage;

    /// Marks `url` as drawn now, so the host keeps its download while a screen shows it. The
    /// host's record of use is the only one; the atlas keeps no recency of its own.
    fn touch(&self, _url: &str) {}
}

/// One downloaded image's progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteImage {
    Loading,
    /// A body whose generation stays fixed until the host publishes another download.
    Ready(BodyGeneration, Arc<[u8]>),
    Failed,
}

/// Whether `path` names a downloaded image rather than a pack file.
pub fn is_remote(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

/// How the host runs the atlas's image work. Without workers every read and decode runs on the
/// calling thread, so a host that cannot start threads still gets every image.
#[derive(Clone)]
pub struct ImageWork {
    /// Monotonic time since any fixed origin, bounding each frame's inline decodes; `None`
    /// leaves them unbounded. Without workers, misses past the bound wait for the next frame.
    pub clock: Option<Arc<dyn Fn() -> Duration + Send + Sync>>,
    /// Whether large reads, large decodes and over-budget decodes may run on background threads.
    pub workers: bool,
}

impl ImageWork {
    /// Every read and decode on the calling thread, unbounded.
    pub fn inline() -> Self {
        Self {
            clock: None,
            workers: false,
        }
    }

    /// The end of a frame's inline decode window starting now; `None` when unbounded.
    pub(crate) fn deadline(&self, budget: Duration) -> Option<Duration> {
        self.clock.as_ref().map(|clock| clock() + budget)
    }

    /// Whether `deadline` has not yet passed.
    pub(crate) fn before(&self, deadline: Option<Duration>) -> bool {
        match (&self.clock, deadline) {
            (Some(clock), Some(deadline)) => clock() < deadline,
            _ => true,
        }
    }
}

/// Background workers with unbounded inline decodes, until a host installs its own.
impl Default for ImageWork {
    fn default() -> Self {
        Self {
            clock: None,
            workers: true,
        }
    }
}
