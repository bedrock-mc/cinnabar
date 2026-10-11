//! Focus regions retain the native landmark hierarchy independently of pointer clipping.

use super::super::super::{UiPresentationError, rect};
use super::paint::{Bounds, Canvas};
use launcher::menu::{
    MenuAction,
    view::{SettingsFocusAxis, SettingsFocusLandmark},
};

#[cfg(test)]
mod tests;

pub(super) const SCREEN: u16 = 0;
pub(super) const HEADER: u16 = 1;
pub(super) const CONTENT: u16 = 2;
pub(super) const SIDEBAR: u16 = 3;
pub(super) const DETAIL: u16 = 4;
pub(super) const PICKER: u16 = 5;
pub(super) const PICKER_HEADER: u16 = 6;
pub(super) const PICKER_BODY: u16 = 7;
pub(super) const SIDEBAR_SCROLL: u16 = 8;
pub(super) const PICKER_SCROLL: u16 = 9;
const TAB_BASE: u16 = 16;

pub(super) fn tab(section: u8) -> u16 {
    TAB_BASE + u16::from(section) * 2
}

impl Canvas<'_> {
    pub(super) fn begin_focus_region(
        &mut self,
        id: u16,
        bounds: Bounds,
        scroll_axis: Option<SettingsFocusAxis>,
        remember: bool,
    ) -> Result<Option<u16>, UiPresentationError> {
        let parent = self.focus_parent;
        self.focus_landmarks.push(SettingsFocusLandmark {
            id,
            parent,
            bounds: rect(bounds[0], bounds[1], bounds[2], bounds[3])?,
            scroll_axis,
            delegate: None,
            delegate_landmark: None,
            remember,
            trap: false,
            focus_control_disabled: false,
        });
        self.focus_parent = Some(id);
        Ok(parent)
    }

    pub(super) fn focus_delegate(&mut self, action: Option<MenuAction>, landmark: Option<u16>) {
        if let Some(region) = self
            .focus_landmarks
            .iter_mut()
            .find(|region| Some(region.id) == self.focus_parent)
        {
            region.delegate = action;
            region.delegate_landmark = landmark;
        }
    }

    pub(super) fn focus_trap(&mut self) {
        if let Some(region) = self
            .focus_landmarks
            .iter_mut()
            .find(|region| Some(region.id) == self.focus_parent)
        {
            region.trap = true;
        }
    }

    pub(super) fn disable_focus_delegation(&mut self) {
        if let Some(region) = self
            .focus_landmarks
            .iter_mut()
            .find(|region| Some(region.id) == self.focus_parent)
        {
            region.focus_control_disabled = true;
        }
    }

    pub(super) fn end_focus_region(&mut self, parent: Option<u16>) {
        self.focus_parent = parent;
    }

    pub(super) fn clear_focus_geometry(&mut self) {
        self.focus_hits.clear();
        self.focus_targets.clear();
        self.focus_landmarks.clear();
        self.focus_parent = None;
    }
}
