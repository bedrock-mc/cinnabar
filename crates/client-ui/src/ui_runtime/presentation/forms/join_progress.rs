//! World and Realms join progress screens, with titles and messages
//! for connection, resource pack downloads, and world generation.

use json_ui::{DataSource, Scalar};

use super::menu_screens::{Translate, flags, text, translated};
use launcher::menu::{JoinKind, JoinProgress, JoinStage};

/// Vanilla's world join progress screen.
const WORLD_SCREEN: &str = "progress.world_loading_progress_screen";
/// Vanilla's Realm join progress screen.
const REALM_SCREEN: &str = "progress.realms_stories_loading_progress_screen";

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// What the screen shows for one join state.
#[derive(Debug, PartialEq)]
pub(super) struct Shown {
    pub(super) title: String,
    pub(super) message: String,
    /// The determinate bar's clipped-away fraction; `None` draws the animated bar.
    pub(super) clipped: Option<f64>,
    pub(super) cancel: bool,
}

/// Binds `join`'s title, message, bar and cancel button; returns the screen to open.
pub(super) fn bind(
    join: &JoinProgress,
    data: &mut DataSource,
    translate: Translate<'_>,
) -> &'static str {
    let tr = |key: &str, fallback: &str| translated(translate, key, fallback);
    let shown = shown(join, &tr);
    data.set_global("#title_text", text(shown.title));
    data.set_global("#progress_text", text(shown.message));
    match shown.clipped {
        Some(clipped) => {
            flags(data, &["#loading_bar_visible"]);
            // The bar binds this as its `#clip_ratio`.
            data.set_global("#loading_bar_percentage", Scalar::Num(clipped));
        }
        None => flags(data, &["#bar_animation_visible"]),
    }
    if shown.cancel {
        flags(data, &["#cancel_visible"]);
        data.set_global("#cancel_button_text", text(tr("gui.cancel", "Cancel")));
    }
    match join.kind {
        JoinKind::Realm => REALM_SCREEN,
        JoinKind::External | JoinKind::Local => WORLD_SCREEN,
    }
}

pub(super) fn shown(join: &JoinProgress, tr: &impl Fn(&str, &str) -> String) -> Shown {
    let (title, message) = match join.stage {
        JoinStage::Realm => (
            tr("realmJoining.progressTitle", "Joining Realm..."),
            tr(
                "progressScreen.message.waitingForRealms",
                "This may take a few moments",
            ),
        ),
        JoinStage::Connecting => (connect_title(join.kind, tr), String::new()),
        JoinStage::Packs { total_bytes: 0, .. } => (
            tr(
                "progressScreen.title.applyingPacks",
                "Loading resource packs",
            ),
            String::new(),
        ),
        JoinStage::Packs {
            done,
            total,
            received_bytes,
            total_bytes,
        } => {
            let title = tr("progressScreen.title.downloading", "Downloading packs %1")
                .replace("%1", &format!("[{done} / {total}]"));
            let size = |bytes| file_size(bytes, tr);
            let base = format!("[{} / {}]", size(received_bytes), size(total_bytes));
            let message = match (join.bytes_per_sec(), join.eta_secs()) {
                (Some(rate), Some(eta)) if total_bytes > received_bytes => {
                    let stats = tr("progressScreen.message.downloadStats", "%1/s, %2 left")
                        .replace("%1", &size(rate))
                        .replace("%2", &format_eta(eta));
                    format!("{base}\n{stats}")
                }
                _ => base,
            };
            (title, message)
        }
        JoinStage::Generating => (
            tr("progressScreen.generating", "Generating World"),
            tr("progressScreen.message.locating", "Locating server"),
        ),
    };
    let clipped = match join.stage {
        JoinStage::Packs {
            received_bytes,
            total_bytes,
            ..
        } if total_bytes > 0 => {
            Some(1.0 - received_bytes.min(total_bytes) as f64 / total_bytes as f64)
        }
        _ => None,
    };
    Shown {
        title,
        message,
        clipped,
        cancel: join.cancellable(),
    }
}

fn connect_title(kind: JoinKind, tr: &impl Fn(&str, &str) -> String) -> String {
    match kind {
        JoinKind::External => tr(
            "progressScreen.title.connectingExternal",
            "Connecting to external server",
        ),
        JoinKind::Realm => tr(
            "progressScreen.title.connectingRealms",
            "Connecting to Realm",
        ),
        JoinKind::Local => tr("progressScreen.title.connectingLocal", "Starting World"),
    }
}

/// Vanilla file sizes: MB to two places under 1 MiB and one above, GB from 1 GiB.
fn file_size(bytes: u64, tr: &impl Fn(&str, &str) -> String) -> String {
    let megabytes = || tr("playscreen.fileSize.MB", "MB");
    if bytes < MIB {
        format!("{:.2}{}", bytes as f64 / MIB as f64, megabytes())
    } else if bytes < GIB {
        format!("{:.1}{}", bytes as f64 / MIB as f64, megabytes())
    } else {
        let gigabytes = tr("playscreen.fileSize.GB", "GB");
        format!("{:.1}{gigabytes}", bytes as f64 / GIB as f64)
    }
}

/// Short ETA for the download line: seconds, minutes and seconds, or hours and minutes.
fn format_eta(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        launcher::menu::{JoinKind, JoinProgress, JoinStage},
    };

    fn tr(_: &str, fallback: &str) -> String {
        fallback.to_owned()
    }

    fn join(kind: JoinKind, stage: JoinStage) -> JoinProgress {
        let mut join = JoinProgress::new(kind);
        join.observe(Some(stage));
        join
    }

    fn words(join: &JoinProgress) -> (String, String) {
        let shown = shown(join, &tr);
        (shown.title, shown.message)
    }

    const APPLYING: JoinStage = JoinStage::Packs {
        done: 0,
        total: 0,
        received_bytes: 0,
        total_bytes: 0,
    };

    // Each stage carries vanilla's title and message, and Realm joins open the Realms screen.
    #[test]
    fn stages_read_as_vanilla_words_them() {
        let connecting = JoinProgress::new(JoinKind::External);
        assert_eq!(
            words(&connecting),
            ("Connecting to external server".into(), String::new())
        );
        assert!(shown(&connecting, &tr).cancel);
        let realm = join(JoinKind::Realm, JoinStage::Realm);
        assert_eq!(
            words(&realm),
            (
                "Joining Realm...".into(),
                "This may take a few moments".into()
            )
        );
        assert!(!shown(&realm, &tr).cancel);
        let realm_connect = join(JoinKind::Realm, JoinStage::Connecting);
        assert_eq!(words(&realm_connect).0, "Connecting to Realm");
        assert_eq!(
            words(&JoinProgress::new(JoinKind::Local)).0,
            "Starting World"
        );
        let applying = join(JoinKind::External, APPLYING);
        assert_eq!(words(&applying).0, "Loading resource packs");
        assert_eq!(shown(&applying, &tr).clipped, None);
        let mut generating = applying;
        generating.observe(None);
        assert_eq!(
            words(&generating),
            ("Generating World".into(), "Locating server".into())
        );
        let screen = |join: &JoinProgress| bind(join, &mut DataSource::new(), &|_| None);
        assert_eq!(screen(&realm), REALM_SCREEN);
        assert_eq!(screen(&connecting), WORLD_SCREEN);
    }

    // A download titles the pack count, sizes the bytes and fills the bar across all packs.
    #[test]
    fn a_pack_download_fills_the_bar() {
        let downloading = join(
            JoinKind::External,
            JoinStage::Packs {
                done: 1,
                total: 3,
                received_bytes: 5 * MIB,
                total_bytes: 20 * MIB,
            },
        );
        assert_eq!(
            shown(&downloading, &tr),
            Shown {
                title: "Downloading packs [1 / 3]".into(),
                message: "[5.0MB / 20.0MB]".into(),
                clipped: Some(0.75),
                cancel: true,
            }
        );
    }

    #[test]
    fn file_sizes_follow_util_get_filesize_string() {
        assert_eq!(file_size(512 * 1024, &tr), "0.50MB");
        assert_eq!(file_size(MIB + MIB / 5, &tr), "1.2MB");
        assert_eq!(file_size(3 * GIB / 2, &tr), "1.5GB");
    }

    // Once the rate steadies, the download adds its speed and time left.
    #[test]
    fn a_steady_download_shows_rate_and_eta() {
        use std::time::{Duration, Instant};

        let stage = |received| JoinStage::Packs {
            done: 0,
            total: 5,
            received_bytes: received,
            total_bytes: 20 * MIB,
        };
        let mut downloading = JoinProgress::new(JoinKind::External);
        let start = Instant::now();
        downloading.observe_at(Some(stage(0)), start);
        downloading.observe_at(Some(stage(5 * MIB)), start + Duration::from_secs(2));
        assert_eq!(
            shown(&downloading, &tr),
            Shown {
                title: "Downloading packs [0 / 5]".into(),
                message: "[5.0MB / 20.0MB]\n2.5MB/s, 6s left".into(),
                clipped: Some(0.75),
                cancel: true,
            }
        );
    }

    #[test]
    fn etas_read_as_short_durations() {
        assert_eq!(format_eta(5), "5s");
        assert_eq!(format_eta(65), "1m 05s");
        assert_eq!(format_eta(3665), "1h 01m");
    }
}
