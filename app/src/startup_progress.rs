//! What the client is doing until its first frame draws, for a page that waits on it.
//!
//! A browser tab cannot repaint while the client starts on its only thread, so its page shows
//! these reports over the canvas and hides them once the game draws.

use std::cell::RefCell;

/// One report: what the client is doing, how far along when known, and whether it is done.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartupProgress<'a> {
    pub label: &'a str,
    pub fraction: Option<f32>,
    pub ready: bool,
}

thread_local! {
    static REPORTER: RefCell<Option<Box<dyn Fn(&StartupProgress<'_>)>>> = const { RefCell::new(None) };
}

/// Sends every later report to `reporter`; a client with none, as native ones, reports nothing.
pub fn set_reporter(reporter: impl Fn(&StartupProgress<'_>) + 'static) {
    REPORTER.with_borrow_mut(|slot| *slot = Some(Box::new(reporter)));
}

/// Reports that the client is `label`, `fraction` of the way through when known.
pub(crate) fn report(label: &str, fraction: Option<f32>) {
    send(&StartupProgress {
        label,
        fraction,
        ready: false,
    });
}

fn send(progress: &StartupProgress<'_>) {
    REPORTER.with_borrow(|slot| {
        if let Some(reporter) = slot {
            reporter(progress);
        }
    });
}

/// Reports the UI texture pages' upload until a frame draws with all of them, then that the
/// client is ready.
#[cfg(target_arch = "wasm32")]
pub(crate) fn report_ui_upload(
    stats: bevy::prelude::Res<render_scene::UiRenderStatsResource>,
    mut ready: bevy::prelude::Local<bool>,
) {
    if *ready {
        return;
    }
    let snapshot = stats.0.snapshot();
    let [uploaded, total] = snapshot.texture_upload;
    if total > 0 && uploaded == total && snapshot.accepted_revision.is_some() {
        *ready = true;
        send(&StartupProgress {
            label: "Ready",
            fraction: Some(1.0),
            ready: true,
        });
    } else if total > 0 {
        report(
            "Preparing the interface",
            Some(uploaded as f32 / total as f32),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn reports_reach_the_installed_reporter_only() {
        report("before any reporter", None);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        set_reporter(move |progress| {
            sink.borrow_mut()
                .push((progress.label.to_owned(), progress.fraction, progress.ready));
        });
        report("Loading game files", None);
        report("Preparing the interface", Some(0.5));
        assert_eq!(
            *seen.borrow(),
            [
                ("Loading game files".to_owned(), None, false),
                ("Preparing the interface".to_owned(), Some(0.5), false),
            ]
        );
    }
}
