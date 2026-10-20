//! Bounded placement history, independent of guest-authored card content.

use super::*;
use std::collections::{HashSet, VecDeque};
use ui::mod_hud::Placement;

const HISTORY_LIMIT: usize = 64;

#[derive(Clone, PartialEq)]
struct Layout {
    placements: Vec<Placement>,
    hidden: HashSet<String>,
}

impl Layout {
    /// Captures authored placement values without cloning card content or native caches.
    fn capture(hud: &Hud, hidden: &HashSet<String>) -> Self {
        Self {
            placements: hud.cards.iter().map(Placement::capture).collect(),
            hidden: hidden.clone(),
        }
    }

    /// Restores matching IDs while leaving republished content and newly added cards intact.
    fn restore(&self, hud: &mut Hud, hidden: &mut HashSet<String>) -> bool {
        let mut resized = false;
        for card in &mut hud.cards {
            if let Some(placement) = self.placements.iter().find(|p| p.id == card.id) {
                resized |= card.scale != placement.scale;
                placement.apply_to(card);
                if self.hidden.contains(&card.id) {
                    hidden.insert(card.id.clone());
                } else {
                    hidden.remove(&card.id);
                }
            }
        }
        resized
    }
}

pub(super) struct History {
    current: Layout,
    undo: VecDeque<Layout>,
    redo: Vec<Layout>,
}

impl History {
    /// Seeds the first state when the editor opens.
    pub(super) fn new(hud: &Hud) -> Self {
        Self {
            current: Layout::capture(hud, &HashSet::new()),
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Adds one completed edit, discarding redo only when the placement actually changed.
    pub(super) fn record(&mut self, hud: &Hud, hidden: &HashSet<String>) {
        let next = Layout::capture(hud, hidden);
        if next == self.current {
            return;
        }
        self.undo
            .push_back(std::mem::replace(&mut self.current, next));
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// Reports which direction the toolbar and keyboard may step.
    pub(super) fn available(&self) -> [bool; 2] {
        [!self.undo.is_empty(), !self.redo.is_empty()]
    }

    /// Reconciles live IDs without promoting an unfinished gesture to the completed baseline.
    pub(super) fn refresh(&mut self, hud: &Hud, hidden: &HashSet<String>) {
        let mut next = Layout::capture(hud, hidden);
        for placement in &mut next.placements {
            if let Some(old) = self
                .current
                .placements
                .iter()
                .find(|old| old.id == placement.id)
            {
                placement.clone_from(old);
                if self.current.hidden.contains(&placement.id) {
                    next.hidden.insert(placement.id.clone());
                } else {
                    next.hidden.remove(&placement.id);
                }
            }
        }
        self.current = next;
    }

    /// Restores one completed edit without recording the restoration as a new edit.
    fn step(&mut self, redo: bool, hud: &mut Hud, hidden: &mut HashSet<String>) -> Option<bool> {
        let next = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        }?;
        let previous = std::mem::replace(&mut self.current, next);
        if redo {
            self.undo.push_back(previous);
        } else {
            self.redo.push(previous);
        }
        let resized = self.current.restore(hud, hidden);
        self.current = Layout::capture(hud, hidden);
        Some(resized)
    }
}

impl HudEditor {
    /// Steps history, drops transient feedback and persists the resulting draft when opted in.
    pub(super) fn step_history(&mut self, redo: bool) {
        self.cancel_pointer_input();
        self.settle_anchor_motion();
        if let Some(resized) = self.history.step(redo, &mut self.draft, &mut self.hidden) {
            if resized {
                self.catalog = None;
            }
            self.toast = Default::default();
            self.data_dirty = true;
            self.reset = false;
            self.persist();
        }
    }
}
