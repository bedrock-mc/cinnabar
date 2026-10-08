//! Screen-level `ScreenSettings` and the scene-stack rules that read them, after
//! 1.26.50 `UIControlFactory` (screen branch), `UIScene` and `SceneStack`.

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;

/// A screen root's settings. Defaults are vanilla's parser fallbacks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenSettings {
    pub screen_not_flushable: bool,
    pub always_accepts_input: bool,
    pub render_game_behind: bool,
    pub absorbs_input: bool,
    pub is_showing_menu: bool,
    pub is_modal: bool,
    pub should_steal_mouse: bool,
    pub low_frequency_rendering: bool,
    pub screen_draws_last: bool,
    /// The scene below still counts as topmost.
    pub force_render_below: bool,
    pub send_telemetry: bool,
    pub close_on_player_hurt: bool,
    pub use_custom_pocket_toast: bool,
    pub cache_screen: bool,
    pub gamepad_cursor: bool,
    pub gamepad_cursor_deflection_mode: bool,
    /// `None` keeps the scroll view's own step.
    pub vertical_scroll_delta: Option<f32>,
    pub load_screen_immediately: bool,
    pub render_only_when_topmost: bool,
    pub should_be_skipped_during_automation: bool,
}

impl Default for ScreenSettings {
    fn default() -> Self {
        Self {
            screen_not_flushable: false,
            always_accepts_input: false,
            render_game_behind: true,
            absorbs_input: true,
            is_showing_menu: true,
            is_modal: false,
            should_steal_mouse: false,
            low_frequency_rendering: false,
            screen_draws_last: false,
            force_render_below: false,
            send_telemetry: true,
            close_on_player_hurt: false,
            use_custom_pocket_toast: false,
            cache_screen: false,
            gamepad_cursor: false,
            gamepad_cursor_deflection_mode: false,
            vertical_scroll_delta: None,
            load_screen_immediately: false,
            render_only_when_topmost: true,
            should_be_skipped_during_automation: false,
        }
    }
}

impl ScreenSettings {
    /// Read scene policy from the same resolved root used for painting.
    pub fn from_root(root: &crate::ResolvedControl) -> Self {
        Self::from_properties(&root.properties)
    }

    /// Covered screens draw when the pack permits rendering below another scene.
    pub fn renders(self, topmost: bool) -> bool {
        topmost || !self.render_only_when_topmost
    }

    /// Settings from a resolved screen root's properties; a value that is not a
    /// bool (an unbound `$var`) keeps the default, as vanilla does for null.
    pub fn from_properties(properties: &BTreeMap<String, Value>) -> Self {
        let d = Self::default();
        let flag = |key: &str, default: bool| match properties.get(key) {
            Some(Value::Bool(value)) => *value,
            Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
            _ => default,
        };
        Self {
            screen_not_flushable: flag("screen_not_flushable", d.screen_not_flushable),
            always_accepts_input: flag("always_accepts_input", d.always_accepts_input),
            render_game_behind: flag("render_game_behind", d.render_game_behind),
            absorbs_input: flag("absorbs_input", d.absorbs_input),
            is_showing_menu: flag("is_showing_menu", d.is_showing_menu),
            is_modal: flag("is_modal", d.is_modal),
            should_steal_mouse: flag("should_steal_mouse", d.should_steal_mouse),
            low_frequency_rendering: flag("low_frequency_rendering", d.low_frequency_rendering),
            screen_draws_last: flag("screen_draws_last", d.screen_draws_last),
            force_render_below: flag("force_render_below", d.force_render_below),
            send_telemetry: flag("send_telemetry", d.send_telemetry),
            close_on_player_hurt: flag("close_on_player_hurt", d.close_on_player_hurt),
            use_custom_pocket_toast: flag("use_custom_pocket_toast", d.use_custom_pocket_toast),
            cache_screen: flag("cache_screen", d.cache_screen),
            gamepad_cursor: flag("gamepad_cursor", d.gamepad_cursor),
            gamepad_cursor_deflection_mode: flag(
                "gamepad_cursor_deflection_mode",
                d.gamepad_cursor_deflection_mode,
            ),
            vertical_scroll_delta: properties
                .get("vertical_scroll_delta")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            load_screen_immediately: flag("load_screen_immediately", d.load_screen_immediately),
            render_only_when_topmost: flag("render_only_when_topmost", d.render_only_when_topmost),
            should_be_skipped_during_automation: flag(
                "should_be_skipped_during_automation",
                d.should_be_skipped_during_automation,
            ),
        }
    }
}

/// One scene on a [`SceneStack`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneEntry<K> {
    pub key: K,
    pub settings: ScreenSettings,
}

/// Live scenes bottom first, answering what draws, what gets input and who owns the mouse.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneStack<K> {
    scenes: Vec<SceneEntry<K>>,
}

impl<K> Default for SceneStack<K> {
    fn default() -> Self {
        Self { scenes: Vec::new() }
    }
}

impl<K: Copy + PartialEq> SceneStack<K> {
    pub fn push(&mut self, key: K, settings: ScreenSettings) {
        self.scenes.push(SceneEntry { key, settings });
    }

    pub fn scenes(&self) -> &[SceneEntry<K>] {
        &self.scenes
    }

    pub fn top(&self) -> Option<&SceneEntry<K>> {
        self.scenes.last()
    }

    pub fn contains(&self, key: K) -> bool {
        self.scenes.iter().any(|scene| scene.key == key)
    }

    /// Scenes in paint order: from the highest scene that
    /// hides the game up, skipping topmost-only scenes under the effective top,
    /// then every draws-last scene. `frequent_only` is the per-frame pass that
    /// also skips menu and low-frequency scenes.
    pub fn visible(&self, frequent_only: bool) -> Vec<K> {
        let count = self.scenes.len();
        let highest = |test: fn(&ScreenSettings) -> bool| {
            (1..count)
                .rev()
                .find(|&index| test(&self.scenes[index].settings))
                .unwrap_or(0)
        };
        let top = highest(|settings| !settings.force_render_below);
        let bottom = highest(|settings| !settings.render_game_behind);
        let mut out = Vec::new();
        for (start, draws_last) in [(bottom, false), (0, true)] {
            for (index, scene) in self.scenes.iter().enumerate().skip(start) {
                let settings = &scene.settings;
                let hidden = index < top && settings.render_only_when_topmost;
                let skip = if frequent_only {
                    settings.is_showing_menu
                        || settings.low_frequency_rendering
                        || hidden
                        || settings.screen_draws_last != draws_last
                } else {
                    hidden || settings.screen_draws_last != draws_last
                };
                if !skip {
                    out.push(scene.key);
                }
            }
        }
        out
    }

    /// Scenes an input event reaches, topmost first: down through scenes that
    /// pass input on to the first that absorbs it, then any below that always
    /// accept input.
    pub fn input_targets(&self) -> Vec<K> {
        let mut out = Vec::new();
        let mut absorbed = false;
        for scene in self.scenes.iter().rev() {
            if !absorbed {
                out.push(scene.key);
                absorbed = scene.settings.absorbs_input;
            } else if scene.settings.always_accepts_input {
                out.push(scene.key);
            }
        }
        out
    }

    pub fn receives_input(&self, key: K) -> bool {
        self.input_targets().contains(&key)
    }

    /// Whether the top screen captures the mouse, as vanilla decides.
    pub fn steals_mouse(&self) -> bool {
        self.top()
            .is_some_and(|scene| scene.settings.should_steal_mouse)
    }

    /// Whether vanilla would treat the top screen as a menu.
    pub fn showing_menu(&self) -> bool {
        self.top()
            .is_some_and(|scene| scene.settings.is_showing_menu)
    }

    /// The top scene when it closes on player damage.
    pub fn closes_on_hurt(&self) -> Option<K> {
        self.top()
            .filter(|scene| scene.settings.close_on_player_hurt)
            .map(|scene| scene.key)
    }
}

#[derive(Debug)]
enum NavOp<K> {
    Push(K),
    Pop(usize),
    PopExpecting(Vec<K>),
}

/// A navigation history of screens with `SceneStack`'s immediate and scheduled
/// push/pop operations; scheduled ones apply on the next [`ScreenNav::update`].
#[derive(Debug)]
pub struct ScreenNav<K> {
    stack: Vec<K>,
    scheduled: VecDeque<NavOp<K>>,
}

impl<K> Default for ScreenNav<K> {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            scheduled: VecDeque::new(),
        }
    }
}

impl<K: Copy + PartialEq> ScreenNav<K> {
    pub fn top(&self) -> Option<K> {
        self.stack.last().copied()
    }

    pub fn screens(&self) -> &[K] {
        &self.stack
    }

    pub fn push(&mut self, screen: K) {
        self.stack.push(screen);
    }

    pub fn pop(&mut self) -> Option<K> {
        self.stack.pop()
    }

    /// Replaces the history with one root screen.
    pub fn reset(&mut self, root: K) {
        self.stack.clear();
        self.scheduled.clear();
        self.stack.push(root);
    }

    /// Pops back to the first instance of `screen` and returns true, or leaves the
    /// history alone when it holds none.
    pub fn pop_back_to(&mut self, screen: K) -> bool {
        match self.stack.iter().position(|entry| *entry == screen) {
            Some(index) => {
                self.stack.truncate(index + 1);
                true
            }
            None => false,
        }
    }

    /// Pops every screen `keep` rejects as flushable.
    pub fn flush(&mut self, keep: impl Fn(K) -> bool) {
        self.stack.retain(|screen| keep(*screen));
        self.scheduled.clear();
    }

    pub fn schedule_push(&mut self, screen: K) {
        self.scheduled.push_back(NavOp::Push(screen));
    }

    /// Queues `count` pops, never more than the history and earlier pops leave.
    pub fn schedule_pop(&mut self, count: usize) {
        let available = self.stack.len().saturating_sub(self.scheduled_pops());
        let count = count.min(available);
        if count > 0 {
            self.scheduled.push_back(NavOp::Pop(count));
        }
    }

    /// Queues pops that apply only while the top screens match `expected`, topmost first.
    pub fn schedule_pop_expecting(&mut self, expected: Vec<K>) {
        self.scheduled.push_back(NavOp::PopExpecting(expected));
    }

    fn scheduled_pops(&self) -> usize {
        self.scheduled
            .iter()
            .map(|op| match op {
                NavOp::Pop(count) => *count,
                NavOp::PopExpecting(expected) => expected.len(),
                NavOp::Push(_) => 0,
            })
            .sum()
    }

    pub fn has_scheduled(&self) -> bool {
        !self.scheduled.is_empty()
    }

    /// Applies the scheduled operations in order.
    pub fn update(&mut self) {
        while let Some(op) = self.scheduled.pop_front() {
            match op {
                NavOp::Push(screen) => self.stack.push(screen),
                NavOp::Pop(count) => {
                    let keep = self.stack.len().saturating_sub(count);
                    self.stack.truncate(keep);
                }
                NavOp::PopExpecting(expected) => {
                    let matches = expected.len() <= self.stack.len()
                        && self.stack.iter().rev().zip(&expected).all(|(a, b)| a == b);
                    if matches {
                        let keep = self.stack.len() - expected.len();
                        self.stack.truncate(keep);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
