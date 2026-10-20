//! Feedback while the selected shader pack loads: a toast stands as a slow load translates its
//! programs, an outcome toast replaces it once the pack is ready or fails, and a failure
//! explains itself in the Video Renderer row. Callers pass the render host's load state and
//! their monotonic clock each frame; each state change raises at most one toast.

use std::{fmt::Write, time::Duration};

/// How long a load runs before its toast stands, so a pack that loads at once only reports
/// how it went.
pub const LOADING_TOAST_DELAY: Duration = Duration::from_millis(500);
/// Characters of a failure's first line an outcome toast carries.
const SUMMARY_CHARS: usize = 96;
/// Failure lines the Renderer row lists before counting the rest.
const DESCRIBED_LINES: usize = 6;

/// A pack's display name: its file or folder name without a `.zip` extension.
pub fn pack_label(name: &str) -> &str {
    name.len()
        .checked_sub(".zip".len())
        .filter(|&split| {
            split > 0
                && name
                    .get(split..)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(".zip"))
        })
        .and_then(|split| name.get(..split))
        .unwrap_or(name)
}

/// The render host's load state, naming the pack by its file or folder name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackLoad<'a> {
    /// No pack requested: Vanilla draws.
    Idle,
    /// `programs` is `(translated, total)` once the load knows its program count.
    Loading {
        pack: &'a str,
        programs: Option<(u32, u32)>,
    },
    Ready {
        pack: &'a str,
    },
    /// `reason` holds one problem per line, the first summarising the failure.
    Failed {
        pack: &'a str,
        reason: &'a str,
    },
}

/// A change to the shader-pack toasts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackToast {
    /// Stand the loading toast, or update its lines while it stands.
    Loading { title: String, message: String },
    /// Take the loading toast down and show how the load went for the toast duration.
    Outcome { title: String, message: String },
    /// Take the loading toast down: the pack was turned off before it loaded.
    Withdraw,
}

/// Turns load-state transitions into toasts.
#[derive(Debug, Default)]
pub struct PackNotices {
    seen: Seen,
    /// The load in progress: when its toast may stand and the progress it last showed.
    loading: Option<LoadingToast>,
}

#[derive(Debug, Default)]
enum Seen {
    /// Vanilla, as at startup: nothing to report.
    #[default]
    Idle,
    Loading(String),
    Ready(String),
    Failed {
        pack: String,
        reason: String,
    },
}

impl Seen {
    fn is(&self, load: PackLoad<'_>) -> bool {
        match (self, load) {
            (Self::Idle, PackLoad::Idle) => true,
            (Self::Loading(seen), PackLoad::Loading { pack, .. })
            | (Self::Ready(seen), PackLoad::Ready { pack }) => seen == pack,
            (
                Self::Failed {
                    pack: seen,
                    reason: why,
                },
                PackLoad::Failed { pack, reason },
            ) => seen == pack && why == reason,
            _ => false,
        }
    }

    fn of(load: PackLoad<'_>) -> Self {
        match load {
            PackLoad::Idle => Self::Idle,
            PackLoad::Loading { pack, .. } => Self::Loading(pack.to_owned()),
            PackLoad::Ready { pack } => Self::Ready(pack.to_owned()),
            PackLoad::Failed { pack, reason } => Self::Failed {
                pack: pack.to_owned(),
                reason: reason.to_owned(),
            },
        }
    }
}

#[derive(Debug)]
struct LoadingToast {
    stands_at: Duration,
    /// The progress on the standing toast; `None` until it stands.
    shown: Option<Option<(u32, u32)>>,
}

impl PackNotices {
    /// The toast change `load` calls for at `now`, if any: one per state change, plus
    /// progress updates to a standing loading toast. A load that settles within
    /// [`LOADING_TOAST_DELAY`] reports only its outcome. `can_stand` is false while another
    /// toast holds the slot; unchanged progress is retried once the slot is available.
    pub fn observe(
        &mut self,
        load: PackLoad<'_>,
        now: Duration,
        can_stand: bool,
    ) -> Option<PackToast> {
        if !self.seen.is(load) {
            self.seen = Seen::of(load);
            let standing = self
                .loading
                .take()
                .is_some_and(|loading| loading.shown.is_some());
            match load {
                PackLoad::Idle => return standing.then_some(PackToast::Withdraw),
                PackLoad::Ready { pack } => {
                    return Some(PackToast::Outcome {
                        title: format!("Shader pack {} loaded", pack_label(pack)),
                        message: String::new(),
                    });
                }
                PackLoad::Failed { pack, reason } => {
                    return Some(PackToast::Outcome {
                        title: format!("Shader pack {} failed to load", pack_label(pack)),
                        message: failure_summary(reason),
                    });
                }
                PackLoad::Loading { .. } => {
                    // A pack picked while another loads keeps the standing toast up.
                    let delay = if standing {
                        Duration::ZERO
                    } else {
                        LOADING_TOAST_DELAY
                    };
                    self.loading = Some(LoadingToast {
                        stands_at: now.saturating_add(delay),
                        shown: None,
                    });
                }
            }
        }
        let (PackLoad::Loading { pack, programs }, Some(loading)) = (load, self.loading.as_mut())
        else {
            return None;
        };
        if !can_stand {
            loading.shown = None;
            return None;
        }
        if now < loading.stands_at || loading.shown == Some(programs) {
            return None;
        }
        loading.shown = Some(programs);
        Some(PackToast::Loading {
            title: format!("Loading shader pack {}…", pack_label(pack)),
            message: progress_line(programs),
        })
    }
}

/// The loading toast's second line.
fn progress_line(programs: Option<(u32, u32)>) -> String {
    match programs {
        Some((translated, total)) if translated < total => {
            format!("Translated {translated} of {total} programs")
        }
        Some(_) => "Building pipelines".to_owned(),
        None => String::new(),
    }
}

/// The failure's first line, shortened for a toast, counting the other lines.
pub fn failure_summary(reason: &str) -> String {
    let mut lines = reason.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next().unwrap_or_default().trim();
    let mut summary: String = first.chars().take(SUMMARY_CHARS).collect();
    if summary.len() < first.len() {
        summary.push('…');
    }
    let more = lines.count();
    if more > 0 {
        let _ = write!(summary, " (+{more} more)");
    }
    summary
}

/// The Renderer row's description while the selected pack has failed: the reason's lines,
/// the first [`DESCRIBED_LINES`] of them, then a count of the rest.
pub fn failure_description(reason: &str) -> String {
    let mut text = String::from("This pack failed to load, so Vanilla draws instead.");
    let mut lines = reason.lines().filter(|line| !line.trim().is_empty());
    for line in lines.by_ref().take(DESCRIBED_LINES) {
        text.push('\n');
        text.push_str(line.trim());
    }
    let more = lines.count();
    if more > 0 {
        let _ = write!(text, "\n…and {more} more in the client log.");
    }
    text
}

#[cfg(test)]
mod tests;
