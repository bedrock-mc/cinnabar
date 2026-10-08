//! Header clicks disclose rows; captured drags commit one stable section move on release.

use launcher::menu::server_list::{ServerGroup, ServerListAction};
use ui::UiPoint;

use super::super::paint::Bounds;
use crate::menu::{MenuAction, MenuScreen};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
struct Section {
    group: ServerGroup,
    header: Bounds,
    bottom: f32,
}

#[derive(Clone, Copy)]
struct Capture {
    group: ServerGroup,
    anchor: UiPoint,
    pointer: UiPoint,
    grab: f32,
    dragging: bool,
    seconds: f64,
}

#[derive(Clone, Copy)]
pub(in super::super) struct Preview {
    pub group: ServerGroup,
    pub bounds: Bounds,
    pub marker: Option<f32>,
}

#[derive(Default)]
pub(in super::super) struct DragState {
    viewport: Option<Bounds>,
    sections: Vec<Section>,
    capture: Option<Capture>,
    touched: bool,
}

fn contains(b: Bounds, p: UiPoint) -> bool {
    p.x() >= b[0] && p.x() <= b[2] && p.y() >= b[1] && p.y() <= b[3]
}

impl DragState {
    /// A captured pointer may move a section or trigger scrolling on its next frame.
    pub(in crate::ui_runtime::presentation) fn captured(&self) -> bool {
        self.capture.is_some()
    }

    pub(in super::super) fn begin_layout(&mut self, viewport: Bounds) {
        self.viewport = Some(viewport);
        self.sections.clear();
        self.touched = true;
    }

    pub(in super::super) fn section(&mut self, group: ServerGroup, header: Bounds, bottom: f32) {
        self.sections.push(Section {
            group,
            header,
            bottom,
        });
    }

    pub(in super::super) fn end_frame(&mut self) {
        if !std::mem::take(&mut self.touched) {
            self.cancel();
            self.viewport = None;
            self.sections.clear();
        }
    }

    pub(in super::super) fn cancel(&mut self) {
        self.capture = None;
    }

    pub(in super::super) fn dragged_group(&self) -> Option<ServerGroup> {
        self.capture.filter(|c| c.dragging).map(|c| c.group)
    }

    fn destination(&self, capture: Capture) -> Option<(Option<ServerGroup>, f32)> {
        let viewport = self.viewport?;
        if !contains(viewport, capture.pointer) {
            return None;
        }
        let mut last = None;
        for section in self.sections.iter().filter(|s| s.group != capture.group) {
            if capture.pointer.y() < (section.header[1] + section.header[3]) * 0.5 {
                return Some((Some(section.group), section.header[1]));
            }
            last = Some(section.bottom);
        }
        last.map(|bottom| (None, bottom))
    }

    pub(in super::super) fn preview(&self) -> Option<Preview> {
        let capture = self.capture.filter(|c| c.dragging)?;
        let section = self.sections.iter().find(|s| s.group == capture.group)?;
        let viewport = self.viewport?;
        let height = section.header[3] - section.header[1];
        let top = (capture.pointer.y() - capture.grab).clamp(viewport[1], viewport[3] - height);
        Some(Preview {
            group: capture.group,
            bounds: [section.header[0], top, section.header[2], top + height],
            marker: self.destination(capture).map(|(_, y)| y),
        })
    }

    /// Returns pointer ownership, a release action, and edge-scroll pixels.
    fn pointer(
        &mut self,
        point: Option<UiPoint>,
        held: bool,
        pressed: bool,
        seconds: f64,
    ) -> (bool, Option<MenuAction>, f32) {
        if pressed
            && self.capture.is_none()
            && let Some(point) = point.filter(|p| self.viewport.is_some_and(|v| contains(v, *p)))
            && let Some(section) = self.sections.iter().find(|s| contains(s.header, point))
        {
            self.capture = Some(Capture {
                group: section.group,
                anchor: point,
                pointer: point,
                grab: point.y() - section.header[1],
                dragging: false,
                seconds,
            });
        }
        let Some(mut capture) = self.capture else {
            return (false, None, 0.0);
        };
        let Some(point) = point else {
            self.cancel();
            return (true, None, 0.0);
        };
        capture.pointer = point;
        let distance =
            (point.x() - capture.anchor.x()).powi(2) + (point.y() - capture.anchor.y()).powi(2);
        capture.dragging |= distance >= 36.0;
        if !held {
            let action = if capture.dragging {
                self.destination(capture)
                    .map(|(before, _)| ServerListAction::MoveBefore(capture.group, before))
            } else {
                self.sections
                    .iter()
                    .find(|s| s.group == capture.group)
                    .filter(|s| {
                        contains(s.header, point)
                            && self.viewport.is_some_and(|v| contains(v, point))
                    })
                    .map(|_| ServerListAction::Toggle(capture.group))
            };
            self.cancel();
            return (true, action.map(MenuAction::ServerList), 0.0);
        }
        let elapsed = (seconds - capture.seconds).clamp(0.0, 0.05) as f32;
        capture.seconds = seconds;
        self.capture = Some(capture);
        let scroll = self
            .viewport
            .filter(|v| capture.dragging && point.x() >= v[0] && point.x() <= v[2])
            .map_or(0.0, |v| {
                let edge = 36.0;
                let up = ((v[1] + edge - point.y()) / edge).clamp(0.0, 1.0);
                let down = ((point.y() - v[3] + edge) / edge).clamp(0.0, 1.0);
                (up - down) * elapsed * 420.0
            });
        (true, None, scroll)
    }
}

impl crate::ui_runtime::presentation::UiPresentationRuntime {
    pub fn cancel_menu_server_list_input(&mut self) {
        self.form_presentation
            .oreui_transitions
            .server_list
            .cancel();
    }

    /// Section capture suppresses click activation until release, including outside the list.
    pub fn menu_server_list_pointer(
        &mut self,
        screen: Option<MenuScreen>,
        pointer: Option<UiPoint>,
        held: bool,
        pressed: bool,
    ) -> (bool, Option<MenuAction>) {
        let state = &mut self.form_presentation.oreui_transitions.server_list;
        if !matches!(screen, Some(MenuScreen::Servers | MenuScreen::Play)) {
            state.cancel();
            return (false, None);
        }
        let (captured, action, scroll) = state.pointer(pointer, held, pressed, self.menu_seconds);
        if scroll != 0.0 {
            self.menu_scrolls.scroll_by("servers.side_menu", scroll);
        }
        (captured, action)
    }
}
