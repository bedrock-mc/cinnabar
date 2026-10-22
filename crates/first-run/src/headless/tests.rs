use std::{cell::RefCell, rc::Rc};

use super::*;
use crate::{consent_marker, test_support::Dir};

const SERVER_DOWNLOAD_PROGRESS: &str = "Downloading local world server: 4 / 8 MB";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    Consent,
    Assets,
    LocalWorlds,
    Progress(String),
}

type Events = Rc<RefCell<Vec<Event>>>;

#[derive(Clone, Copy, Default)]
enum Completion {
    #[default]
    Success,
    Cancelled,
    CancelAfterSuccess,
    Failed,
}

struct Fixture {
    pending: Pending,
    events: Events,
    assets: Completion,
    local_worlds: Completion,
}

impl Fixture {
    /// Tracks preparation and progress in one ordered event stream.
    fn new(assets: bool, local_worlds: bool) -> Self {
        Self {
            pending: Pending {
                assets,
                local_worlds,
            },
            events: Events::default(),
            assets: Completion::Success,
            local_worlds: Completion::Success,
        }
    }
}

impl Preparer for Fixture {
    fn pending(&self, _: &InstallLayout) -> Pending {
        self.pending
    }

    fn assets(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        report: &mut dyn FnMut(Status),
    ) -> Result<()> {
        assert!(consent_recorded(layout));
        self.events.borrow_mut().push(Event::Assets);
        report(Status::new(Phase::Running, 1, 1, "Preparing assets"));
        match self.assets {
            Completion::Success => {}
            Completion::Cancelled => return Err(runner::Cancelled.into()),
            Completion::CancelAfterSuccess => cancel.store(true, Ordering::Relaxed),
            Completion::Failed => return Err(anyhow!("asset preparation failed")),
        }
        self.pending.assets = false;
        Ok(())
    }

    fn local_worlds(
        &mut self,
        layout: &InstallLayout,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), WorldError> {
        assert!(consent_recorded(layout));
        assert_ne!(last_phase(layout), "done");
        self.events.borrow_mut().push(Event::LocalWorlds);
        progress(SERVER_DOWNLOAD_PROGRESS);
        progress(LOCAL_WORLDS_LABEL);
        match self.local_worlds {
            Completion::Success => {}
            Completion::Cancelled => return Err(WorldError::Cancelled),
            Completion::CancelAfterSuccess => cancel.store(true, Ordering::Relaxed),
            Completion::Failed => {
                return Err(WorldError::Failed("local preparation detail".into()));
            }
        }
        self.pending.local_worlds = false;
        Ok(())
    }
}

/// Uses the Android install layout with temporary private roots on every test host.
fn layout(data: &Dir) -> InstallLayout {
    InstallLayout::android(data.path(), data.path().join("native/core.so")).with_prepared_assets()
}

/// Reads the status Android setup publishes for the current attempt.
fn last_phase(layout: &InstallLayout) -> String {
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(crate::progress_file(layout)).unwrap()).unwrap();
    status["phase"].as_str().unwrap().to_owned()
}

/// Records both the consent callback and the progress shown by the headless entry point.
fn attempt(
    layout: &InstallLayout,
    fixture: &mut Fixture,
    cancel: &AtomicBool,
    accept: bool,
) -> Result<Outcome> {
    let consent_events = Rc::clone(&fixture.events);
    let progress_events = Rc::clone(&fixture.events);
    prepare_with(
        layout,
        cancel,
        |_, _| {
            consent_events.borrow_mut().push(Event::Consent);
            accept
        },
        |line| {
            progress_events
                .borrow_mut()
                .push(Event::Progress(line.to_owned()));
        },
        fixture,
    )
}

/// Whether setup told the activity it could start the game.
fn is_ready(event: &Event) -> bool {
    matches!(event, Event::Progress(line) if line == &Status::ready().summary())
}

#[test]
fn current_assets_stale_local_worlds_prepare_before_ready() {
    let data = Dir::new("headless-worlds-only");
    let layout = layout(&data);
    let mut fixture = Fixture::new(false, true);
    let outcome = attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap();

    assert_eq!(outcome, Outcome::Prepared);
    assert!(!fixture.pending.any());
    let events = fixture.events.borrow();
    assert!(!events.contains(&Event::Assets));
    let worlds = events
        .iter()
        .position(|event| *event == Event::LocalWorlds)
        .unwrap();
    let ready = events.iter().position(is_ready).unwrap();
    assert!(worlds < ready);
    assert_eq!(events.iter().filter(|event| is_ready(event)).count(), 1);
    assert!(events.contains(&Event::Progress(LOCAL_WORLDS_LABEL.into())));
    assert!(events.contains(&Event::Progress(SERVER_DOWNLOAD_PROGRESS.into())));
    assert_eq!(last_phase(&layout), "done");
}

#[test]
fn stale_assets_finish_before_local_worlds_and_ready() {
    let data = Dir::new("headless-both-stale");
    let layout = layout(&data);
    let mut fixture = Fixture::new(true, true);
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap(),
        Outcome::Prepared
    );
    let events = fixture.events.borrow();
    let assets = events
        .iter()
        .position(|event| *event == Event::Assets)
        .unwrap();
    let worlds = events
        .iter()
        .position(|event| *event == Event::LocalWorlds)
        .unwrap();
    let ready = events.iter().position(is_ready).unwrap();
    assert!(assets < worlds && worlds < ready);
    assert_eq!(events.iter().filter(|event| is_ready(event)).count(), 1);
}

#[test]
fn current_assets_and_local_worlds_skip_consent_and_all_work() {
    let data = Dir::new("headless-current");
    let layout = layout(&data);
    let mut fixture = Fixture::new(false, false);
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), false).unwrap(),
        Outcome::NotNeeded
    );
    assert!(fixture.events.borrow().is_empty());
    assert!(!consent_marker(&layout).exists());
    assert!(!crate::progress_file(&layout).exists());
}

#[test]
fn declined_consent_runs_neither_preparation_stage() {
    let data = Dir::new("headless-declined-stages");
    let layout = layout(&data);
    let mut fixture = Fixture::new(true, true);
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), false).unwrap(),
        Outcome::Quit
    );
    let events = fixture.events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::Consent)
            .count(),
        1
    );
    assert!(!events.contains(&Event::Assets));
    assert!(!events.contains(&Event::LocalWorlds));
    assert!(!events.iter().any(is_ready));
    assert!(!consent_marker(&layout).exists());
}

#[test]
fn changed_consent_requires_acceptance_before_local_worlds() {
    let data = Dir::new("headless-worlds-consent");
    let layout = layout(&data);
    crate::record_consent(&layout).unwrap();
    fs::write(consent_marker(&layout), "previous terms").unwrap();
    let mut fixture = Fixture::new(false, true);
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), false).unwrap(),
        Outcome::Quit
    );
    assert!(fixture.events.borrow().contains(&Event::Consent));
    assert!(!fixture.events.borrow().contains(&Event::LocalWorlds));
    assert!(!consent_recorded(&layout));
}

#[test]
fn recorded_consent_covers_local_worlds_without_another_prompt() {
    let data = Dir::new("headless-worlds-accepted");
    let layout = layout(&data);
    crate::record_consent(&layout).unwrap();
    let mut fixture = Fixture::new(false, true);
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), false).unwrap(),
        Outcome::Prepared
    );
    assert!(!fixture.events.borrow().contains(&Event::Consent));
    assert!(fixture.events.borrow().contains(&Event::LocalWorlds));
}

#[test]
fn cancellation_before_setup_skips_everything_even_when_current() {
    for pending in [false, true] {
        let data = Dir::new("headless-already-cancelled");
        let layout = layout(&data);
        let mut fixture = Fixture::new(pending, pending);
        assert_eq!(
            attempt(&layout, &mut fixture, &AtomicBool::new(true), true).unwrap(),
            Outcome::Quit
        );
        assert!(fixture.events.borrow().is_empty());
    }
}

#[test]
fn asset_cancellation_never_prepares_local_worlds_or_reports_ready() {
    for completion in [Completion::Cancelled, Completion::CancelAfterSuccess] {
        let data = Dir::new("headless-assets-cancel");
        let layout = layout(&data);
        let mut fixture = Fixture::new(true, true);
        fixture.assets = completion;
        assert_eq!(
            attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap(),
            Outcome::Quit
        );
        let events = fixture.events.borrow();
        assert!(events.contains(&Event::Assets));
        assert!(!events.contains(&Event::LocalWorlds));
        assert!(!events.iter().any(is_ready));
        assert_ne!(last_phase(&layout), "done");
    }
}

#[test]
fn local_world_cancellation_quits_without_ready() {
    for completion in [Completion::Cancelled, Completion::CancelAfterSuccess] {
        let data = Dir::new("headless-worlds-cancel");
        let layout = layout(&data);
        let mut fixture = Fixture::new(false, true);
        fixture.local_worlds = completion;
        assert_eq!(
            attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap(),
            Outcome::Quit
        );
        let events = fixture.events.borrow();
        assert!(events.contains(&Event::LocalWorlds));
        assert!(!events.iter().any(is_ready));
        assert_ne!(last_phase(&layout), "done");
    }
}

#[test]
fn cancelling_from_local_world_progress_does_not_start_the_stage() {
    let data = Dir::new("headless-worlds-progress-cancel");
    let layout = layout(&data);
    let mut fixture = Fixture::new(false, true);
    let cancel = AtomicBool::new(false);
    let mut progress = Vec::new();
    assert_eq!(
        prepare_with(
            &layout,
            &cancel,
            |_, _| true,
            |line| {
                if line == LOCAL_WORLDS_LABEL {
                    cancel.store(true, Ordering::Relaxed);
                }
                progress.push(Event::Progress(line.into()));
            },
            &mut fixture,
        )
        .unwrap(),
        Outcome::Quit
    );
    assert!(!fixture.events.borrow().contains(&Event::LocalWorlds));
    assert!(!progress.iter().any(is_ready));
}

#[test]
fn asset_failure_blocks_local_worlds_and_readiness() {
    let data = Dir::new("headless-assets-fail");
    let layout = layout(&data);
    let mut fixture = Fixture::new(true, true);
    fixture.assets = Completion::Failed;
    attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap_err();
    let events = fixture.events.borrow();
    assert!(events.contains(&Event::Assets));
    assert!(!events.contains(&Event::LocalWorlds));
    assert!(!events.iter().any(is_ready));
    assert_eq!(last_phase(&layout), "failed");
}

#[test]
fn local_world_failure_retries_setup_without_rebuilding_current_assets() {
    let data = Dir::new("headless-worlds-retry");
    let layout = layout(&data);
    let mut fixture = Fixture::new(true, true);
    fixture.local_worlds = Completion::Failed;
    let error = attempt(&layout, &mut fixture, &AtomicBool::new(false), true).unwrap_err();
    assert_eq!(format!("{error:#}"), LOCAL_WORLDS_FAILED);
    assert_eq!(last_phase(&layout), "failed");
    assert!(!fixture.events.borrow().iter().any(is_ready));
    assert!(!fixture.pending.assets && fixture.pending.local_worlds);
    assert!(consent_recorded(&layout));
    let log = fs::read_to_string(prepare::log_path(&layout)).unwrap();
    assert!(log.contains("local preparation detail"));

    fixture.local_worlds = Completion::Success;
    assert_eq!(
        attempt(&layout, &mut fixture, &AtomicBool::new(false), false).unwrap(),
        Outcome::Prepared
    );
    assert_eq!(last_phase(&layout), "done");
    let events = fixture.events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::Assets)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::LocalWorlds)
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::Consent)
            .count(),
        1
    );
    assert_eq!(events.iter().filter(|event| is_ready(event)).count(), 1);
}

#[cfg(not(target_os = "android"))]
#[test]
fn desktop_setup_has_no_local_world_preparation_requirement() {
    let data = Dir::new("desktop-headless");
    assert!(!local_worlds_needed(&layout(&data)));
}
