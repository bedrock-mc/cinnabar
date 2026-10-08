//! Owner-requested motion uses short finite timelines, retaining only mounted surfaces.

use super::{theme::Rgba, widgets::Interaction};
use crate::menu::{MenuAction, MenuScreen};

#[cfg(test)]
mod tests;

const HIGHLIGHT_SECONDS: f64 = 0.085;
const SURFACE_HIGHLIGHT_SECONDS: f64 = 0.120;
const PRESS_SECONDS: f64 = 0.045;
const RELEASE_SECONDS: f64 = 0.080;
const ENTRANCE_SECONDS: f64 = 0.120;
const DISCLOSURE_SECONDS: f64 = 0.160;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Surface {
    Screen(MenuScreen),
    Settings(u8),
    ResourcePack(u128, bool),
    Wardrobe(bool),
    Play(u8),
    ServerDetails(usize, bool),
    Inbox(u8),
    InboxMessage(u64),
    InboxSettings,
    World(launcher::local_worlds::Screen),
    WorldTab(launcher::local_worlds::Screen, launcher::local_worlds::Tab),
    LoadingStatus(u64),
    Dialog(u64),
    Loading,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Button,
    Surface,
    Tab,
    Field,
    Thumb,
    Radio,
    Disclosure,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Feedback {
    pub(super) hover: f32,
    pub(super) press: f32,
    pub(super) focus: f32,
    pub(super) selected: f32,
}

impl Feedback {
    pub(super) fn immediate(state: Interaction, enabled: bool, selected: bool) -> Self {
        Self {
            hover: u8::from(enabled && state.hovered && !state.pressed) as f32,
            press: u8::from(enabled && state.pressed) as f32,
            focus: u8::from(enabled && state.focused) as f32,
            selected: u8::from(selected) as f32,
        }
    }

    pub(super) fn color(self, idle: Rgba, hovered: Rgba, pressed: Rgba) -> Rgba {
        mix(mix(idle, hovered, self.hover), pressed, self.press)
    }

    pub(super) fn depression(self) -> f32 {
        self.selected + (1.0 - self.selected) * self.press
    }
}

pub(super) fn mix(from: Rgba, to: Rgba, progress: f32) -> Rgba {
    std::array::from_fn(|i| {
        (f32::from(from[i]) + (f32::from(to[i]) - f32::from(from[i])) * progress).round() as u8
    })
}

pub(super) fn opacity(mut color: Rgba, alpha: f32) -> Rgba {
    color[3] = (f32::from(color[3]) * alpha.clamp(0.0, 1.0)).round() as u8;
    color
}

#[derive(Clone, Copy)]
pub(super) struct Tween {
    from: f32,
    target: f32,
    started: f64,
    duration: f64,
}

impl Tween {
    /// Time still changes this channel until its target is reached.
    pub(in crate::ui_runtime::presentation) fn active(self, seconds: f64) -> bool {
        self.from != self.target && seconds - self.started < self.duration
    }

    pub(super) fn at(value: f32) -> Self {
        Self {
            from: value,
            target: value,
            started: 0.0,
            duration: 0.0,
        }
    }

    pub(super) fn sample(self, seconds: f64) -> f32 {
        if self.from == self.target || seconds - self.started >= self.duration {
            return self.target;
        }
        let t = ((seconds - self.started) / self.duration).clamp(0.0, 1.0) as f32;
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(3))
    }

    pub(super) fn retarget(&mut self, target: f32, duration: f64, seconds: f64) -> f32 {
        let current = self.sample(seconds);
        if target != self.target {
            self.from = current;
            self.target = target;
            self.started = seconds;
            self.duration = duration;
        }
        self.sample(seconds)
    }
}

struct Control {
    surface: Surface,
    action: MenuAction,
    kind: Kind,
    channels: [Tween; 4],
    touched: bool,
}

struct Entrance {
    surface: Surface,
    started: Option<f64>,
    touched: bool,
}

pub(super) struct Motion {
    enabled: bool,
    primed: bool,
    controls: Vec<Control>,
    entrances: Vec<Entrance>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            enabled: true,
            primed: false,
            controls: Vec::new(),
            entrances: Vec::new(),
        }
    }
}

impl Motion {
    /// Retention waits for every mounted control and entrance to settle.
    pub(in crate::ui_runtime::presentation) fn active(&self, seconds: f64) -> bool {
        self.enabled
            && (self
                .controls
                .iter()
                .any(|c| c.channels.iter().any(|t| t.active(seconds)))
                || self.entrances.iter().any(|e| {
                    e.started
                        .is_some_and(|start| seconds - start < ENTRANCE_SECONDS)
                }))
    }

    pub(super) fn configure(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.primed = false;
            self.controls.clear();
            for entrance in &mut self.entrances {
                entrance.started = None;
            }
        }
        self.enabled = enabled;
    }

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn feedback(
        &mut self,
        surface: Surface,
        action: Option<MenuAction>,
        kind: Kind,
        target: Feedback,
        seconds: f64,
    ) -> Feedback {
        let Some(action) = action.filter(|_| self.enabled) else {
            return target;
        };
        let action = match (kind, action) {
            (Kind::Thumb, MenuAction::SettingsOption(index, _)) => {
                MenuAction::SettingsOption(index, 0)
            }
            (Kind::Thumb, MenuAction::SettingsFullscreen(_)) => {
                MenuAction::SettingsFullscreen(false)
            }
            (_, action) => action,
        };
        let values = [target.hover, target.press, target.focus, target.selected];
        let index = self.controls.iter().position(|control| {
            control.surface == surface && control.action == action && control.kind == kind
        });
        let index = match index {
            Some(index) => index,
            None if values == [0.0; 4] && kind != Kind::Disclosure => return target,
            None => {
                self.controls.push(Control {
                    surface,
                    action,
                    kind,
                    channels: std::array::from_fn(|index| {
                        Tween::at(if self.primed && kind != Kind::Disclosure {
                            0.0
                        } else {
                            values[index]
                        })
                    }),
                    touched: false,
                });
                self.controls.len() - 1
            }
        };
        let control = &mut self.controls[index];
        control.touched = true;
        let sampled: [f32; 4] = std::array::from_fn(|index| {
            let duration = match index {
                3 if kind == Kind::Disclosure => DISCLOSURE_SECONDS,
                1 if values[index] > 0.0 => PRESS_SECONDS,
                1 => RELEASE_SECONDS,
                _ if kind == Kind::Surface || kind == Kind::Disclosure => SURFACE_HIGHLIGHT_SECONDS,
                _ => HIGHLIGHT_SECONDS,
            };
            control.channels[index].retarget(values[index], duration, seconds)
        });
        Feedback {
            hover: sampled[0],
            press: sampled[1],
            focus: sampled[2],
            selected: sampled[3],
        }
    }

    pub(super) fn entrance(&mut self, surface: Surface, seconds: f64) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let index = self
            .entrances
            .iter()
            .position(|entry| entry.surface == surface)
            .unwrap_or_else(|| {
                self.entrances.push(Entrance {
                    surface,
                    started: self.primed.then_some(seconds),
                    touched: false,
                });
                self.entrances.len() - 1
            });
        let entry = &mut self.entrances[index];
        entry.touched = true;
        let Some(start) = entry.started else {
            return 1.0;
        };
        let t = ((seconds - start) / ENTRANCE_SECONDS).clamp(0.0, 1.0) as f32;
        if t == 1.0 {
            entry.started = None;
        }
        1.0 - (1.0 - t).powi(3)
    }

    pub(super) fn end_frame(&mut self, seconds: f64) {
        self.primed = true;
        self.controls.retain_mut(|control| {
            std::mem::take(&mut control.touched)
                && (control.kind == Kind::Disclosure
                    || control
                        .channels
                        .iter()
                        .any(|channel| channel.target != 0.0 || channel.sample(seconds) != 0.0))
        });
        self.entrances
            .retain_mut(|entry| std::mem::take(&mut entry.touched));
    }
}
