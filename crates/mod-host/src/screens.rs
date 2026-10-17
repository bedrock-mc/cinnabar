//! What a player mod package's screens exchange with the host: the events its callbacks
//! receive, the screens it commits, and the declarations a package carries.

use crate::{CallbackOutcome, ModHost, package::Package, runtime::Declared};
use anyhow::Result;
use experience_sdk::mod_manifest::KeyDecl;
use server_experience::{
    screen::{self, ScreenLayout},
    session_data::{SessionData, Stack},
};
use std::sync::Arc;

/// The modifier keys held with an input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// A session source a `data-changed` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataSource {
    Items,
    Recipes,
}

/// One host event for a `player-mod` component's callbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum ModEvent {
    ScreenChanged(Option<ScreenLayout>),
    Action {
        id: String,
        index: Option<u32>,
    },
    SecondaryAction {
        id: String,
        index: Option<u32>,
    },
    Scrolled {
        delta: f64,
        x: f64,
        y: f64,
        modifiers: KeyModifiers,
    },
    TextChanged {
        control: String,
        text: String,
    },
    /// `row` is the collection and index of the mod's control under the pointer.
    Key {
        id: String,
        hovered: Option<Stack>,
        row: Option<(String, u32)>,
    },
    /// The host closed the view: Escape, or the container closing.
    ViewClosed,
    /// The sources that changed, each once, in order.
    DataChanged(Vec<DataSource>),
}

impl ModEvent {
    /// One frame's events as delivered: the latest layout first, one data change, then the rest
    /// in order, with one text change per edit box (its latest, where it last occurred).
    pub fn coalesce(events: Vec<Self>) -> Vec<Self> {
        let mut out = Vec::with_capacity(events.len());
        let mut layout = None;
        let mut sources = Vec::new();
        let mut data_changed = false;
        // Visiting backwards keeps each text's last occurrence without copying its payload.
        for event in events.into_iter().rev() {
            match event {
                Self::ScreenChanged(_) => {
                    if layout.is_none() {
                        layout = Some(event);
                    }
                }
                Self::DataChanged(mut changed) => {
                    data_changed = true;
                    sources.append(&mut changed);
                }
                Self::TextChanged { ref control, .. }
                    if out.iter().any(|later| {
                        matches!(later, Self::TextChanged { control: other, .. } if other == control)
                    }) => {}
                event => out.push(event),
            }
        }
        out.reverse();
        if data_changed {
            sources.sort();
            sources.dedup();
            out.insert(0, Self::DataChanged(sources));
        }
        if let Some(layout) = layout {
            out.insert(0, layout);
        }
        out
    }
}

/// What a mod draws beside the container screens: its overlay and view templates and the data
/// bound into both.
#[derive(Clone, Debug, Default)]
pub struct ModScreens {
    pub overlay: Option<String>,
    pub view: Option<String>,
    /// The last `focus-text` request, with the data revision it was made at.
    pub focus: Option<(u64, String)>,
    pub data: screen::Modal,
}

/// A loaded package's declarations and screen files.
pub struct LoadedPackage {
    pub id: String,
    pub keys: Vec<KeyDecl>,
    pub files: Arc<screen::Files>,
}

impl ModHost {
    /// The modifiers held with the events about to be dispatched (`screen.held-modifiers`).
    pub fn set_held_modifiers(&mut self, held: KeyModifiers) {
        self.instance.set_held_modifiers(held);
    }

    /// Delivers coalesced events, retaining lifecycle notifications until they actually run.
    /// Latest edit text remains scoped to its screen; key, click and scroll edges are not replayed.
    pub fn dispatch(&mut self, events: Vec<ModEvent>) -> Result<CallbackOutcome> {
        let result = self.dispatch_coalesced(events);
        if !self.is_active() {
            self.pending_events.clear();
        }
        self.queue_settings();
        result
    }

    /// Retries pending lifecycle state before current events without replaying old input.
    fn dispatch_coalesced(&mut self, mut events: Vec<ModEvent>) -> Result<CallbackOutcome> {
        if !self.is_active() {
            self.pending_events.clear();
            return Ok(CallbackOutcome::Executed);
        }
        self.scope_text_changes(&mut events)?;
        let mut incoming = std::mem::take(&mut self.pending_events);
        incoming.extend(events);
        let mut incoming = ModEvent::coalesce(incoming).into_iter();
        while let Some(event) = incoming.next() {
            let mut closed = false;
            if let ModEvent::ScreenChanged(layout) = &event {
                self.layout.clone_from(layout);
                closed = layout.is_none() && self.instance.view_open();
                self.instance.set_layout(layout.clone());
            }
            let outcome = self.instance.dispatch(&event)?;
            if outcome == CallbackOutcome::Deferred {
                self.retain_lifecycle(std::iter::once(event).chain(incoming));
                if closed {
                    self.retain_lifecycle(std::iter::once(ModEvent::ViewClosed));
                }
                return Ok(outcome);
            }
            if closed && self.instance.dispatch(&ModEvent::ViewClosed)? == CallbackOutcome::Deferred
            {
                self.retain_lifecycle(std::iter::once(ModEvent::ViewClosed));
            }
        }
        Ok(if self.pending_events.is_empty() {
            CallbackOutcome::Executed
        } else {
            CallbackOutcome::Deferred
        })
    }

    /// Validates edit values and drops queued text when its screen or view departs.
    fn scope_text_changes(&mut self, events: &mut Vec<ModEvent>) -> Result<()> {
        for event in events
            .iter()
            .filter(|event| matches!(event, ModEvent::TextChanged { .. }))
        {
            self.instance.validate_event(event)?;
        }
        let layout = events.iter().rev().find_map(|event| match event {
            ModEvent::ScreenChanged(layout) => Some(layout),
            _ => None,
        });
        let closed = events.contains(&ModEvent::ViewClosed) || layout == Some(&None);
        let same_screen = layout.is_none_or(|next| match (&self.layout, next) {
            (Some(old), Some(new)) => {
                old.screen == new.screen && old.view.is_some() == new.view.is_some()
            }
            (None, None) => true,
            _ => false,
        });
        if closed || !same_screen {
            self.pending_events
                .retain(|event| !matches!(event, ModEvent::TextChanged { .. }));
        }
        if closed {
            events.retain(|event| !matches!(event, ModEvent::TextChanged { .. }));
        }
        Ok(())
    }

    /// Keeps latest lifecycle state and one bounded edit value per declared control.
    fn retain_lifecycle(&mut self, events: impl Iterator<Item = ModEvent>) {
        let mut pending = std::mem::take(&mut self.pending_events);
        for event in events {
            match event {
                ModEvent::ScreenChanged(_)
                | ModEvent::DataChanged(_)
                | ModEvent::TextChanged { .. } => pending.push(event),
                ModEvent::ViewClosed if !pending.contains(&ModEvent::ViewClosed) => {
                    pending.push(event)
                }
                _ => {}
            }
        }
        self.pending_events = ModEvent::coalesce(pending);
    }

    /// The session's items and recipes the guest reads from now on; a changed revision
    /// delivers `data-changed`.
    pub fn set_session(&mut self, session: Arc<SessionData>) -> Result<()> {
        match self.update_session(session) {
            Some(event) => self.dispatch(vec![event]).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Updates the guest's session snapshot and returns its revision event for frame dispatch.
    pub fn update_session(&mut self, session: Arc<SessionData>) -> Option<ModEvent> {
        let sources: Vec<DataSource> = [
            (
                DataSource::Items,
                session.item_revision != self.session.item_revision,
            ),
            (
                DataSource::Recipes,
                session.recipe_revision != self.session.recipe_revision,
            ),
        ]
        .into_iter()
        .filter_map(|(source, changed)| changed.then_some(source))
        .collect();
        self.session = Arc::clone(&session);
        self.instance.set_session(session);
        (!sources.is_empty()).then_some(ModEvent::DataChanged(sources))
    }

    /// Stops current authority immediately, preserving an existing quarantine reason.
    pub fn revoke(&mut self) {
        self.drawing_restart = crate::DrawingRestart::None;
        self.pending_events.clear();
        self.instance.revoke();
    }

    /// Stops the guest as a trap would, removing everything it presented: the host refused
    /// what it asked to draw.
    pub fn quarantine(&mut self) {
        self.drawing_restart = crate::DrawingRestart::None;
        self.pending_events.clear();
        self.instance.quarantine();
    }

    /// Cancels edits for a departing view, delivers its other input, then closes it.
    /// Pointer actions run before closure so they cannot reopen the escaped view.
    pub fn dispatch_and_close_view(&mut self, mut events: Vec<ModEvent>) -> Result<()> {
        self.pending_events
            .retain(|event| !matches!(event, ModEvent::TextChanged { .. }));
        events.retain(|event| !matches!(event, ModEvent::TextChanged { .. }));
        let result = self.dispatch(events).map(|_| ());
        self.close_view()?;
        result
    }

    /// Closes the view as Escape does over it, and tells the guest with `view-closed`.
    pub fn close_view(&mut self) -> Result<()> {
        if self.instance.close_view() {
            self.dispatch(vec![ModEvent::ViewClosed])?;
        }
        Ok(())
    }

    /// The fuel the last `init` or callback consumed: a `player-mod` component's `init` and
    /// `data-changed` get `LOAD_FUEL`, its other events `CALLBACK_FUEL`, `frame` its own.
    pub fn last_fuel_used(&self) -> u64 {
        self.instance.last_fuel()
    }

    /// The committed overlay, view and bound data; empty after a trap.
    pub fn screens(&self) -> &ModScreens {
        self.instance.screens()
    }

    /// The loaded package, when the mod came from one.
    pub fn package(&self) -> Option<&LoadedPackage> {
        self.package.as_ref()
    }

    /// Whether the component exports `player-mod`'s event callbacks.
    pub fn has_events(&self) -> bool {
        self.instance.has_events()
    }
}

pub(crate) fn declared(package: &Package) -> Declared {
    let manifest = &package.manifest;
    Declared {
        templates: manifest.templates.iter().cloned().collect(),
        actions: manifest.actions.iter().cloned().collect(),
        keys: manifest.keys.iter().map(|key| key.id.clone()).collect(),
    }
}

pub(crate) fn loaded(package: Package) -> LoadedPackage {
    LoadedPackage {
        id: package.manifest.id.clone(),
        keys: package.manifest.keys,
        files: package.files,
    }
}
