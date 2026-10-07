//! Control motion uses the menu clock and retires on unmount.

use std::collections::HashMap;

use super::motion::Tween;
use crate::menu::MenuAction;

mod progress;
pub(super) mod resources;
#[cfg(test)]
mod tests;
mod text;
pub(super) use text::Insertion;

const SWITCH_DURATION: f64 = 0.100;
pub(super) const SWITCH_TRAVEL: f32 = 2.8;
const SLIDER_DURATION: f64 = 0.300;
const SELECT_PRESS_DURATION: f64 = 0.150;
pub(super) const ICON_HIGHLIGHT_FRAMES: u16 = 9;
const ICON_HIGHLIGHT_DURATION: f64 = 0.200;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Control {
    Setting(u16),
    Fullscreen,
}

impl Control {
    fn of(action: MenuAction) -> Option<Self> {
        match action {
            MenuAction::SettingsOption(index, _) => Some(Self::Setting(index)),
            MenuAction::SettingsFullscreen(_) => Some(Self::Fullscreen),
            _ => None,
        }
    }
}

pub(super) fn same_switch_action(candidate: Option<MenuAction>, action: MenuAction) -> bool {
    let key = Control::of(action);
    key.is_some() && candidate.and_then(Control::of) == key
}

#[derive(Default)]
pub(in super::super) struct Transitions {
    pub(super) motion: super::motion::Motion,
    pub(super) effects: super::paint::Effects,
    switches: HashMap<(u8, Control), Switch>,
    sliders: HashMap<(u8, u16), Slider>,
    presses: Vec<Press>,
    last_activation: Option<(MenuAction, u64)>,
    mounted: bool,
    seconds: f64,
    section: u8,
    settings_drawn: bool,
    selected_icon: Option<(u8, f64)>,
    play_icon: Option<(u8, f64)>,
    play_drawn: bool,
    inbox_icon: Option<(u8, f64)>,
    inbox_drawn: bool,
    world_icon: Option<(u8, f64)>,
    world_drawn: bool,
    server_icon: Option<(usize, f64)>,
    servers_drawn: bool,
    pub(super) text: text::TextEdits,
    pub(super) progress: progress::ProgressTween,
    pub(super) resources: resources::Resources,
    pub(super) server_list: super::play_servers::drag::DragState,
}

struct Switch {
    value: bool,
    serial: Option<u64>,
    position: Tween,
    visible: bool,
    touched: bool,
}

struct Press {
    action: MenuAction,
    started: f64,
    touched: bool,
}

struct Slider {
    target: f64,
    from: f64,
    started: f64,
    duration: f64,
    reversing_start: f64,
    shortening: f64,
    active: bool,
    touched: bool,
    visible: bool,
}

impl Transitions {
    pub(in crate::ui_runtime::presentation) fn configure_motion(&mut self, enabled: bool) {
        if enabled != self.motion.enabled() {
            self.switches.clear();
            self.sliders.clear();
            self.presses.clear();
            self.text.clear();
            self.progress = Default::default();
            self.resources = Default::default();
        }
        self.motion.configure(enabled);
        self.effects.configure(enabled);
    }

    pub(super) fn begin_settings(&mut self, section: u8) {
        if self
            .selected_icon
            .is_none_or(|(selected, _)| selected != section)
        {
            self.selected_icon = Some((section, self.seconds));
        }
        self.section = section;
        self.settings_drawn = true;
    }

    pub(super) fn icon_frame(&self, section: u8) -> Option<u16> {
        self.highlight_frame(self.selected_icon, section)
    }

    pub(super) fn begin_play(&mut self, selected: u8) {
        if self
            .play_icon
            .is_none_or(|(previous, _)| previous != selected)
        {
            self.play_icon = Some((selected, self.seconds));
        }
        self.play_drawn = true;
    }

    pub(super) fn play_icon_frame(&self, selected: u8) -> Option<u16> {
        self.highlight_frame(self.play_icon, selected)
    }

    pub(super) fn begin_inbox(&mut self, selected: u8) {
        if self
            .inbox_icon
            .is_none_or(|(previous, _)| previous != selected)
        {
            self.inbox_icon = Some((selected, self.seconds));
        }
        self.inbox_drawn = true;
    }

    pub(super) fn inbox_icon_frame(&self, selected: u8) -> Option<u16> {
        self.highlight_frame(self.inbox_icon, selected)
    }

    pub(super) fn begin_world(&mut self, selected: u8) {
        if self
            .world_icon
            .is_none_or(|(previous, _)| previous != selected)
        {
            self.world_icon = Some((selected, self.seconds));
        }
        self.world_drawn = true;
    }

    pub(super) fn world_icon_frame(&self, selected: u8) -> Option<u16> {
        self.highlight_frame(self.world_icon, selected)
    }

    pub(super) fn begin_servers(&mut self, selected: Option<usize>) {
        if self.server_icon.map(|(previous, _)| previous) != selected {
            self.server_icon = selected.map(|index| (index, self.seconds));
        }
        self.servers_drawn = true;
    }

    pub(super) fn server_icon_frame(&self, selected: usize) -> Option<u16> {
        self.highlight_frame(self.server_icon, selected)
    }

    fn highlight_frame<T: Copy + Eq>(
        &self,
        selection: Option<(T, f64)>,
        section: T,
    ) -> Option<u16> {
        if !self.motion.enabled() {
            return None;
        }
        let (selected, started) = selection?;
        if selected != section {
            return None;
        }
        let steps = ICON_HIGHLIGHT_FRAMES - 1;
        let progress = ((self.seconds - started) / ICON_HIGHLIGHT_DURATION).clamp(0.0, 1.0);
        Some((progress * f64::from(steps)).floor() as u16)
    }

    pub(super) fn begin_frame(
        &mut self,
        activation: Option<(MenuAction, u64)>,
        navigation: bool,
        seconds: f64,
    ) {
        self.seconds = seconds;
        if self.mounted
            && navigation
            && activation != self.last_activation
            && let Some((action, _)) = activation
        {
            if let Some(press) = self.presses.iter_mut().find(|press| press.action == action) {
                press.started = seconds;
            } else {
                self.presses.push(Press {
                    action,
                    started: seconds,
                    touched: false,
                });
            }
        }
        self.last_activation = activation;
        self.mounted = true;
    }

    pub(super) fn switch(
        &mut self,
        action: MenuAction,
        value: bool,
        activation: Option<(MenuAction, u64)>,
        seconds: f64,
    ) -> f32 {
        let target = switch_target(value);
        if !self.motion.enabled() {
            return target;
        }
        let Some(key) = Control::of(action) else {
            return target;
        };
        let serial = activation
            .and_then(|(action, serial)| (Control::of(action) == Some(key)).then_some(serial));
        let state = self.switches.entry((self.section, key)).or_insert(Switch {
            value,
            serial,
            position: Tween::at(target),
            visible: true,
            touched: false,
        });
        state.touched = true;
        if !state.visible {
            state.visible = true;
            state.position = Tween::at(target);
        }
        if state.value != value {
            let active = state.position.sample(seconds) != switch_target(state.value);
            if active || serial.is_some() && serial != state.serial {
                state.position.retarget(target, SWITCH_DURATION, seconds);
            } else {
                state.position = Tween::at(target);
            }
            state.value = value;
        }
        if serial.is_some() {
            state.serial = serial;
        }
        state.position.sample(seconds)
    }

    pub(super) fn slider(&mut self, index: u16, target: f32, immediate: bool, seconds: f64) -> f32 {
        if !self.motion.enabled() {
            return target.clamp(0.0, 1.0);
        }
        let target = f64::from(target.clamp(0.0, 1.0));
        let state = self.sliders.entry((self.section, index)).or_insert(Slider {
            target,
            from: target,
            started: seconds,
            duration: SLIDER_DURATION,
            reversing_start: target,
            shortening: 1.0,
            active: false,
            touched: false,
            visible: true,
        });
        state.touched = true;
        if !state.visible {
            state.visible = true;
            state.target = target;
            state.active = false;
        }
        let current = state.sample(seconds);
        if target != state.target {
            if immediate || current == target {
                state.active = false;
            } else {
                let reversing = state.active && target == state.reversing_start;
                let shortening = if reversing {
                    (state.progress(seconds) * state.shortening + 1.0 - state.shortening)
                        .abs()
                        .clamp(0.0, 1.0)
                } else {
                    1.0
                };
                state.reversing_start = if reversing { state.target } else { current };
                state.shortening = shortening;
                state.from = current;
                state.started = seconds;
                state.duration = SLIDER_DURATION * shortening;
                state.active = state.duration > 0.0;
            }
            state.target = target;
        }
        state.sample(seconds) as f32
    }

    pub(super) fn pressed(&mut self, action: MenuAction, seconds: f64) -> bool {
        if !self.motion.enabled() {
            return false;
        }
        if let Some(press) = self.presses.iter_mut().find(|press| press.action == action) {
            press.touched = true;
            return seconds - press.started < SELECT_PRESS_DURATION;
        }
        false
    }

    pub(super) fn pressed_switch(&mut self, action: MenuAction, seconds: f64) -> bool {
        if !self.motion.enabled() {
            return false;
        }
        if let Some(press) = self
            .presses
            .iter_mut()
            .find(|press| same_switch_action(Some(press.action), action))
        {
            press.touched = true;
            return seconds - press.started < SELECT_PRESS_DURATION;
        }
        false
    }

    pub(in super::super) fn end_frame(&mut self) {
        self.motion.end_frame(self.seconds);
        self.text.end_frame();
        self.progress.end_frame();
        self.resources.end_frame();
        self.server_list.end_frame();
        if !std::mem::take(&mut self.play_drawn) {
            self.play_icon = None;
        }
        if !std::mem::take(&mut self.inbox_drawn) {
            self.inbox_icon = None;
        }
        if !std::mem::take(&mut self.world_drawn) {
            self.world_icon = None;
        }
        if !std::mem::take(&mut self.servers_drawn) {
            self.server_icon = None;
        }
        if !self.settings_drawn {
            self.selected_icon = None;
        }
        self.switches.retain(|(section, _), switch| {
            if std::mem::take(&mut switch.touched) {
                return true;
            }
            if self.settings_drawn && *section != self.section {
                switch.position = Tween::at(switch_target(switch.value));
                switch.visible = false;
                return true;
            }
            false
        });
        self.sliders.retain(|(section, _), slider| {
            if std::mem::take(&mut slider.touched) {
                return true;
            }
            if self.settings_drawn && *section != self.section {
                slider.active = false;
                slider.visible = false;
                return true;
            }
            false
        });
        self.presses.retain_mut(|press| {
            std::mem::take(&mut press.touched)
                && self.seconds - press.started < SELECT_PRESS_DURATION
        });
        self.settings_drawn = false;
    }
}

impl Slider {
    fn progress(&self, seconds: f64) -> f64 {
        slider_easing(((seconds - self.started) / self.duration).clamp(0.0, 1.0))
    }

    fn sample(&mut self, seconds: f64) -> f64 {
        if !self.active {
            return self.target;
        }
        if seconds - self.started >= self.duration {
            self.active = false;
            return self.target;
        }
        self.from + (self.target - self.from) * self.progress(seconds)
    }
}

pub(super) fn switch_target(on: bool) -> f32 {
    if on { SWITCH_TRAVEL } else { 0.0 }
}

fn slider_easing(progress: f64) -> f64 {
    if progress <= 0.0 || progress >= 1.0 {
        return progress;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..32 {
        let t = (low + high) * 0.5;
        if bezier(t, 0.39, 0.66) < progress {
            low = t;
        } else {
            high = t;
        }
    }
    bezier((low + high) * 0.5, 1.34, 1.02)
}

fn bezier(t: f64, first: f64, second: f64) -> f64 {
    let other = 1.0 - t;
    3.0 * other * other * t * first + 3.0 * other * t * t * second + t * t * t
}
