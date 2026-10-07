//! Raw input through button mappings to components, as vanilla routes
//! button and pointer events: controls
//! are visited top to bottom (inside the topmost modal panel), each fires its
//! first eligible mapping for the button, and a consuming mapping stops the walk.

use std::collections::HashMap;

use serde_json::Value;

use super::edit::{CharOutcome, type_text};
use super::{ButtonEvent, Components, ScreenEvent, slider, toggle};
use crate::input::{HitKind, HitRegion, InputMode, Mapping, MappingScope, MappingType};
use crate::layout::TextMeasure;
use crate::state::ViewState;

/// A second press within this long (s) and distance (px) is a double press.
const DOUBLE_PRESS_SECONDS: f64 = 0.5;
const DOUBLE_PRESS_DISTANCE: f64 = 10.0;

/// One raw button edge: `id` is the input button (`button.menu_select`, …).
#[derive(Clone, Copy, Debug)]
pub struct ButtonInput<'a> {
    pub id: &'a str,
    pub down: bool,
    pub point: Option<[f64; 2]>,
    pub mode: InputMode,
    pub now: f64,
}

/// The pointer's position this frame and whether its primary button is held.
#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    pub point: Option<[f64; 2]>,
    pub held: bool,
    pub mode: InputMode,
    pub now: f64,
}

/// What one input did: the screen events raised and whether a control consumed it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dispatch {
    pub events: Vec<ScreenEvent>,
    pub consumed: bool,
}

/// The per-screen input state vanilla keeps on its input components.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dispatcher {
    /// Last button state per (control key, mapping index).
    last: HashMap<(String, usize), bool>,
    /// Press count, time and position per (control key, button): `PressStats`.
    presses: HashMap<(String, String), (u32, f64, [f64; 2])>,
    /// Controls the pointer hovers, top first.
    hovered: Vec<String>,
    /// The slider whose track the pointer holds.
    track: Option<String>,
    track_button: Option<String>,
    /// Last play time per (control key, sound entry).
    sounds: HashMap<(String, usize), f64>,
    /// Last controller-direction step per slider key.
    stepped: HashMap<String, f64>,
    /// The control tracking a gesture, and the pointer's last position.
    gesture: Option<(String, Option<[f64; 2]>)>,
    gesture_button: Option<String>,
}

impl Dispatcher {
    /// Deliver one button edge.
    pub fn button(
        &mut self,
        regions: &[HitRegion],
        view: &mut ViewState,
        input: ButtonInput<'_>,
    ) -> Dispatch {
        let mut out = Dispatch::default();
        for region in reachable(regions).rev() {
            // `from_button_id: "any"` hands every button to the controller unmapped.
            if let Some(scope) = region.input.any {
                out.events.push(ScreenEvent::Button(event(
                    region, input.id, input.id, &input, false, scope,
                )));
            }
            let double = self.double_press(region, &input);
            let Some((index, mapping)) = region
                .input
                .mappings
                .iter()
                .enumerate()
                .filter(|(_, mapping)| mapping.from == input.id)
                .filter(|(_, mapping)| double == (mapping.kind == MappingType::DoublePressed))
                .find(|(index, mapping)| self.eligible(region, *index, mapping, view, &input))
            else {
                self.forget_edges(region, input.id);
                continue;
            };
            let previous = self.last.insert((region.key.clone(), index), input.down);
            // Interaction edge: the press, or the release on touch.
            let edge = input.mode != InputMode::Touch;
            let interacted = previous != Some(edge) && input.down == edge;
            let fired = event(
                region,
                &mapping.to,
                input.id,
                &input,
                interacted,
                mapping.scope,
            );
            let previous_down = previous == Some(true);
            self.deliver(
                regions,
                region,
                (mapping, previous_down),
                &fired,
                view,
                &input,
                &mut out,
            );
            out.events.push(ScreenEvent::Button(fired));
            if mapping.consume_event && region.widget.consume {
                out.consumed = true;
                break;
            }
        }
        if !input.down {
            if self.track_button.as_deref() == Some(input.id) {
                self.track = None;
                self.track_button = None;
            }
            if self.gesture_button.as_deref() == Some(input.id) {
                if let Some((key, _)) = self.gesture.take() {
                    write_gesture(&key, [0.0; 2], &mut view.components);
                }
                self.gesture_button = None;
            }
        }
        out
    }

    /// Track the pointer: hover chain, hover mappings and a held slider track.
    pub fn pointer(
        &mut self,
        regions: &[HitRegion],
        view: &mut ViewState,
        input: PointerInput,
    ) -> Dispatch {
        let mut out = Dispatch::default();
        let mut hovered = Vec::new();
        if let Some(point) = input.point.filter(|_| input.mode != InputMode::Gamepad) {
            for region in reachable(regions).rev() {
                if !region.contains(point) || !region.input.hover_enabled {
                    continue;
                }
                if input.mode == InputMode::Touch && region.input.prevent_touch_input {
                    continue;
                }
                hovered.push(region.key.clone());
                if region.input.consume_hover_events {
                    out.consumed = true;
                    break;
                }
            }
        }
        for region in regions {
            let was = self.hovered.contains(&region.key);
            let is = hovered.contains(&region.key);
            if was != is {
                self.hover_changed(regions, region, is, &mut view.components, &input, &mut out);
            }
        }
        view.hovered = hovered.first().cloned();
        // A button losing its hover lets go of its press.
        if input.mode != InputMode::Gamepad
            && view
                .pressed
                .as_ref()
                .is_some_and(|key| !hovered.contains(key))
        {
            view.pressed = None;
        }
        self.hovered = hovered;
        if let (Some((key, last)), Some(point)) = (self.gesture.as_mut(), input.point) {
            let delta = last.map_or([0.0; 2], |last| [point[0] - last[0], point[1] - last[1]]);
            *last = Some(point);
            let key = key.clone();
            write_gesture(&key, delta, &mut view.components);
        }
        if let (Some(key), Some(point), true) = (self.track.clone(), input.point, input.held)
            && let Some(region) = regions.iter().find(|region| region.key == key)
        {
            set_slider_at(region, point, false, &mut view.components, &mut out);
        }
        out
    }

    /// Typed text for the selected edit box and any `always_listening` one.
    pub fn text(
        &mut self,
        regions: &[HitRegion],
        view: &mut ViewState,
        input: &str,
        measure: Option<&dyn TextMeasure>,
    ) -> Dispatch {
        let mut out = Dispatch::default();
        let components = &mut view.components;
        let selected = components.selected().map(str::to_owned);
        for region in reachable(regions).filter(|region| region.enabled) {
            let Some(meta) = &region.widget.edit else {
                continue;
            };
            if selected.as_deref() != Some(region.key.as_str()) && !meta.always_listening {
                continue;
            }
            out.consumed = true;
            let edit = components.edit_mut(&region.key, &meta.text);
            match type_text(meta, edit, input, measure) {
                CharOutcome::Changed => {
                    let text = edit.text.clone();
                    write_text(region, &text, true, components);
                    out.events.push(text_event(region, &text, false));
                }
                CharOutcome::Enter if meta.can_be_deselected => {
                    deselect(region, components, &mut out);
                }
                _ => {}
            }
        }
        out
    }

    /// A screen controller's text for the edit boxes whose `text_box_name` is `name`, as vanilla
    /// sets an edit box's text: it replaces the text, cut to `max_length` characters, keeps each
    /// box's selection, and raises no event.
    pub fn set_edit_text(
        &mut self,
        regions: &[HitRegion],
        view: &mut ViewState,
        name: &str,
        text: &str,
    ) -> Dispatch {
        let components = &mut view.components;
        for region in regions {
            let Some(meta) = &region.widget.edit else {
                continue;
            };
            if meta.name.as_deref() != Some(name) {
                continue;
            }
            let limit = usize::try_from(meta.max_length).unwrap_or(0);
            let cut: String = text.chars().take(limit).collect();
            let selected = components.selected() == Some(region.key.as_str());
            let edit = components.edit_mut(&region.key, &meta.text);
            edit.caret = cut.chars().count();
            edit.text.clone_from(&cut);
            write_text(region, &cut, selected, components);
        }
        Dispatch::default()
    }

    /// A controller direction (`ControllerDirectionEventData`) for the focused control.
    pub fn direction(
        &mut self,
        regions: &[HitRegion],
        view: &mut ViewState,
        stick: [f32; 2],
        now: f64,
    ) -> Dispatch {
        let mut out = Dispatch::default();
        let focused = view.focused.clone();
        let components = &mut view.components;
        let Some(region) = focused
            .as_deref()
            .and_then(|key| reachable(regions).find(|region| region.key == key && region.enabled))
        else {
            return out;
        };
        if let Some(meta) = &region.widget.toggle
            && let Some(checked) =
                toggle::on_direction(meta, stick[0], stick[1], checked(region, components))
        {
            set_toggle(regions, region, checked, true, components, &mut out);
            out.consumed = true;
        }
        if let Some(meta) = &region.widget.slider
            && components.selected() == Some(region.key.as_str())
        {
            let axis = if meta.vertical { stick[1] } else { stick[0] };
            let due = self
                .stepped
                .get(&region.key)
                .is_none_or(|last| now - last >= meta.timeout.unwrap_or(0.0));
            if axis.abs() > 0.5 && due {
                self.stepped.insert(region.key.clone(), now);
                step_slider(
                    region,
                    if axis > 0.0 { 1 } else { -1 },
                    components,
                    &mut out,
                );
                out.consumed = true;
            }
        }
        out
    }

    /// Advance edit-box caret blinks by `delta` seconds; `true` when a caret changed.
    pub fn tick(&mut self, regions: &[HitRegion], view: &mut ViewState, delta: f64) -> bool {
        let components = &mut view.components;
        let Some(key) = components.selected().map(str::to_owned) else {
            return false;
        };
        let Some(region) = regions.iter().find(|region| region.key == key) else {
            return false;
        };
        let Some(target) = region
            .widget
            .edit
            .as_ref()
            .and_then(|meta| meta.text_target.clone())
        else {
            return false;
        };
        let text = region
            .widget
            .edit
            .as_ref()
            .map_or("", |meta| meta.text.as_str())
            .to_owned();
        let edit = components.edit_mut(&key, &text);
        if !edit.tick(delta) {
            return false;
        }
        let shown = edit.caret_shown;
        components.write(&target.0, CARET_PROPERTY, Value::Bool(shown));
        true
    }

    fn double_press(&mut self, region: &HitRegion, input: &ButtonInput<'_>) -> bool {
        let tracked =
            region.input.mappings.iter().any(|mapping| {
                mapping.from == input.id && mapping.kind == MappingType::DoublePressed
            });
        if !tracked {
            return false;
        }
        let point = input.point.unwrap_or([0.0; 2]);
        let stats = self
            .presses
            .entry((region.key.clone(), input.id.to_owned()))
            .or_insert((0, f64::NEG_INFINITY, point));
        if input.down {
            let near = (point[0] - stats.2[0]).hypot(point[1] - stats.2[1]) < DOUBLE_PRESS_DISTANCE;
            let double = stats.0 != 0 && input.now - stats.1 < DOUBLE_PRESS_SECONDS && near;
            *stats = (if double { 2 } else { 1 }, input.now, point);
            return double;
        }
        if stats.0 != 2 {
            return false;
        }
        stats.0 = 0;
        true
    }

    /// Whether a pressed mapping applies, with the focused/global rules.
    fn eligible(
        &self,
        region: &HitRegion,
        index: usize,
        mapping: &Mapping,
        view: &ViewState,
        input: &ButtonInput<'_>,
    ) -> bool {
        let components = &view.components;
        if !mapping.input_mode_condition.admits(input.mode) {
            return false;
        }
        let gamepad = input.mode == InputMode::Gamepad;
        let hot = self.hovered.contains(&region.key) || view.is_focused(&region.key);
        let inside = input.point.is_some_and(|point| region.contains(point));
        // A locked control only answers its global mappings.
        if !region.enabled && mapping.kind != MappingType::Global {
            return false;
        }
        match mapping.kind {
            MappingType::Global => true,
            MappingType::Focused => hot || components.selected() == Some(region.key.as_str()),
            MappingType::Pressed | MappingType::DoublePressed => {
                if mapping.ignore_input_scope && region.enabled {
                    return true;
                }
                // The alternate scope reads the pointer's position, not the hover chain.
                if mapping.alternate_input_scope {
                    return if gamepad { hot } else { inside };
                }
                let over = if gamepad {
                    hot
                } else if hot {
                    inside
                } else {
                    region.input.always_listen_to_input && inside
                };
                over || (!input.down
                    && mapping.button_up_right_of_first_refusal
                    && self.last.get(&(region.key.clone(), index)) == Some(&true))
            }
        }
    }

    fn forget_edges(&mut self, region: &HitRegion, id: &str) {
        for (index, mapping) in region.input.mappings.iter().enumerate() {
            if mapping.from == id {
                self.last.remove(&(region.key.clone(), index));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &mut self,
        regions: &[HitRegion],
        region: &HitRegion,
        (mapping, previous_down): (&Mapping, bool),
        fired: &ButtonEvent,
        view: &mut ViewState,
        input: &ButtonInput<'_>,
        out: &mut Dispatch,
    ) {
        if region.kind == HitKind::Button && region.enabled {
            if input.down {
                view.pressed = Some(region.key.clone());
            } else if view.is_pressed(&region.key) {
                view.pressed = None;
            }
        }
        let components = &mut view.components;
        // Gesture component: its button's hold tracks motion; the release zeroes the deltas.
        if region.widget.gesture.as_deref() == Some(fired.id.as_str()) {
            if input.down {
                self.gesture = Some((region.key.clone(), input.point));
                self.gesture_button = Some(input.id.to_owned());
            } else {
                write_gesture(&region.key, [0.0; 2], components);
                self.gesture = None;
            }
        }
        if !region.enabled {
            return;
        }
        if fired.interacted {
            self.play_sounds(region, &fired.id, input.now, out);
        }
        if let Some(meta) = &region.widget.toggle
            && let Some((state, by_click)) = toggle::on_button(
                meta,
                &fired.id,
                fired.interacted,
                checked(region, components),
            )
        {
            set_toggle(regions, region, state, by_click, components, out);
        }
        if let Some(manager) = &region.widget.toggle_manager
            && fired.interacted
        {
            for member in regions {
                let Some(meta) = &member.widget.toggle else {
                    continue;
                };
                match manager.state_for(meta, member.group_index) {
                    Some(state) => set_toggle(regions, member, state, false, components, out),
                    // Gather republishes each managed toggle's state.
                    None if manager.gathers(meta) => {
                        let state = checked(member, components);
                        set_toggle(regions, member, state, false, components, out);
                    }
                    None => {}
                }
            }
        }
        // A slider manager gathers on the release edge.
        if let Some(manager) = &region.widget.slider_manager
            && !input.down
            && previous_down
        {
            for member in regions {
                if let Some(meta) = &member.widget.slider
                    && manager.manages(meta)
                {
                    let value = current_slider(member, components);
                    let step = meta.is_step().then_some(value.max(0.0) as usize);
                    publish_slider(member, value, step, true, components, out);
                }
            }
        }
        if region.widget.slider.is_some() {
            self.slider_button(region, fired, input, components, out);
        }
        if let Some(meta) = &region.widget.edit
            && fired.interacted
        {
            let selected = components.selected() == Some(region.key.as_str());
            if selected && mapping.handle_deselect && meta.can_be_deselected {
                deselect(region, components, out);
            } else if !selected && mapping.handle_select {
                select(region, components, out);
            }
        }
    }

    /// A slider's response to a button event.
    fn slider_button(
        &mut self,
        region: &HitRegion,
        fired: &ButtonEvent,
        input: &ButtonInput<'_>,
        components: &mut Components,
        out: &mut Dispatch,
    ) {
        let Some(meta) = &region.widget.slider else {
            return;
        };
        let id = Some(fired.id.as_str());
        let selected = components.selected() == Some(region.key.as_str());
        if id == meta.track_button.as_deref() {
            if input.down && self.track.is_none() {
                self.track = Some(region.key.clone());
                self.track_button = Some(input.id.to_owned());
                if let Some(point) = input.point {
                    set_slider_at(region, point, false, components, out);
                }
            } else if !input.down && self.track.as_deref() == Some(region.key.as_str()) {
                if let Some(point) = input.point {
                    set_slider_at(region, point, true, components, out);
                }
                self.track = None;
            }
            return;
        }
        if !fired.interacted {
            return;
        }
        if id == meta.small_decrease_button.as_deref() && selected {
            step_slider(region, -1, components, out);
        } else if id == meta.small_increase_button.as_deref() && selected {
            step_slider(region, 1, components, out);
        } else if id == meta.selected_button.as_deref() {
            set_slider_selected(region, !selected, components);
        } else if id == meta.deselected_button.as_deref() && selected {
            set_slider_selected(region, false, components);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn hover_changed(
        &mut self,
        regions: &[HitRegion],
        region: &HitRegion,
        hovered: bool,
        components: &mut Components,
        input: &PointerInput,
        out: &mut Dispatch,
    ) {
        if hovered {
            // A hover event raises a hover mapping in the Up state: never a press.
            for (to, scope) in &region.input.hover_mappings {
                let probe = ButtonInput {
                    id: to,
                    down: false,
                    point: input.point,
                    mode: input.mode,
                    now: input.now,
                };
                out.events.push(ScreenEvent::Button(event(
                    region, to, "", &probe, false, *scope,
                )));
            }
        }
        if let Some(meta) = &region.widget.toggle
            && hovered
            && region.enabled
            && let Some(state) = toggle::on_hover(meta, checked(region, components))
        {
            set_toggle(regions, region, state, false, components, out);
        }
        if let Some(meta) = &region.widget.slider
            && meta.select_on_hover
            && region.enabled
        {
            set_slider_selected(region, hovered, components);
        }
        if let Some(meta) = &region.widget.edit
            && let (Some(placeholder), Some(color)) =
                (&meta.placeholder, &meta.placeholder_hover_color)
        {
            if hovered {
                components.write(placeholder, "color", color.clone());
            } else {
                components.unwrite(placeholder, "color");
            }
        }
    }

    fn play_sounds(&mut self, region: &HitRegion, id: &str, now: f64, out: &mut Dispatch) {
        let Some(sounds) = &region.widget.sounds else {
            return;
        };
        let request = |sound: &super::sound::Sound| ScreenEvent::Sound {
            name: sound.name.clone(),
            volume: sound.volume,
            pitch: sound.pitch,
        };
        if let Some(sound) = &sounds.shorthand {
            out.events.push(request(sound));
        }
        for (index, entry) in sounds.entries.iter().enumerate() {
            if entry.button_name.as_deref().is_some_and(|name| name != id) {
                continue;
            }
            let key = (region.key.clone(), index);
            if self
                .sounds
                .get(&key)
                .is_some_and(|last| last + entry.min_seconds_between_plays >= now)
            {
                continue;
            }
            self.sounds.insert(key, now);
            out.events.push(request(&entry.sound));
        }
    }
}

/// The gesture bag values a tracking control publishes for its renderer.
fn write_gesture(key: &str, delta: [f64; 2], components: &mut Components) {
    // The pointer is gesture source 3 (`#gesture_delta_source`).
    components.write(key, "#gesture_delta_source", Value::from(3));
    components.write(key, "#gesture_mouse_delta_x", Value::from(delta[0]));
    components.write(key, "#gesture_mouse_delta_y", Value::from(delta[1]));
}

/// The bag property a text target reads for its caret blink.
pub(crate) const CARET_PROPERTY: &str = "#text_edit_caret";

/// Regions that take input: those inside the topmost modal panel, plus any
/// that always listen.
fn reachable(regions: &[HitRegion]) -> impl DoubleEndedIterator<Item = &HitRegion> {
    let modal = regions
        .iter()
        .rev()
        .find(|region| region.kind == HitKind::Modal)
        .map(|region| region.key.clone());
    regions.iter().filter(move |region| match &modal {
        Some(root) => {
            region.modal_root.as_deref() == Some(root.as_str())
                || region.key.starts_with(&format!("{root}/"))
                || region.input.always_listen_to_input
        }
        None => true,
    })
}

fn event(
    region: &HitRegion,
    id: &str,
    from: &str,
    input: &ButtonInput<'_>,
    interacted: bool,
    scope: MappingScope,
) -> ButtonEvent {
    ButtonEvent {
        id: id.to_owned(),
        from: from.to_owned(),
        key: region.key.clone(),
        collection_index: region.collection_index,
        collection: region.collection.clone(),
        down: input.down,
        interacted,
        scope,
    }
}

fn checked(region: &HitRegion, components: &Components) -> bool {
    components
        .bag(&region.key)
        .and_then(|bag| bag.get("#toggle_state"))
        .and_then(Value::as_bool)
        .or(region.checked)
        .unwrap_or(false)
}

/// Sets a toggle's checked state and updates its radio group.
fn set_toggle(
    regions: &[HitRegion],
    region: &HitRegion,
    state: bool,
    by_click: bool,
    components: &mut Components,
    out: &mut Dispatch,
) {
    let Some(meta) = &region.widget.toggle else {
        return;
    };
    components.write(&region.key, "#toggle_state", Value::Bool(state));
    if meta.radio && state {
        for other in regions {
            let same_group = other.widget.toggle.as_ref().is_some_and(|candidate| {
                candidate.radio && candidate.name == meta.name && other.key != region.key
            });
            if same_group {
                components.write(&other.key, "#toggle_state", Value::Bool(false));
            }
        }
    }
    let index = if meta.forced_index >= 0 {
        Some(meta.forced_index as usize)
    } else {
        collection_index(region, meta.grid_collection.as_deref())
    };
    out.events.push(ScreenEvent::Toggle {
        name: meta.name.clone().unwrap_or_default(),
        key: region.key.clone(),
        index,
        checked: state,
        by_click,
    });
}

fn current_slider(region: &HitRegion, components: &Components) -> f64 {
    components
        .bag(&region.key)
        .and_then(|bag| bag.get("#slider_value"))
        .and_then(Value::as_f64)
        .or_else(|| region.widget.slider.as_ref().map(|meta| meta.value))
        .unwrap_or(0.0)
}

fn set_slider_at(
    region: &HitRegion,
    point: [f64; 2],
    finished: bool,
    components: &mut Components,
    out: &mut Dispatch,
) {
    let Some(meta) = &region.widget.slider else {
        return;
    };
    let rect = [region.rect.x, region.rect.y, region.rect.w, region.rect.h];
    let (value, step) = meta.value_at(rect, point);
    publish_slider(region, value, step, finished, components, out);
}

fn step_slider(
    region: &HitRegion,
    direction: i32,
    components: &mut Components,
    out: &mut Dispatch,
) {
    let Some(meta) = &region.widget.slider else {
        return;
    };
    let (value, step) = meta.stepped(current_slider(region, components), direction);
    publish_slider(region, value, step, true, components, out);
}

fn publish_slider(
    region: &HitRegion,
    value: f64,
    step: Option<usize>,
    finished: bool,
    components: &mut Components,
    out: &mut Dispatch,
) {
    let Some(meta) = &region.widget.slider else {
        return;
    };
    components.write(&region.key, "#slider_value", Value::from(value));
    out.events.push(ScreenEvent::Slider {
        name: meta.name.clone().unwrap_or_default(),
        key: region.key.clone(),
        index: collection_index(region, meta.collection.as_deref()),
        value,
        step,
        finished,
    });
}

/// The slider box's selected (indent) state lives on the slider's key.
fn set_slider_selected(region: &HitRegion, selected: bool, components: &mut Components) {
    let current = components.selected() == Some(region.key.as_str());
    if selected != current {
        components.set_selected(selected.then(|| region.key.clone()));
    }
    components.write(
        &region.key,
        slider::SELECTED_PROPERTY,
        Value::Bool(selected),
    );
}

fn select(region: &HitRegion, components: &mut Components, out: &mut Dispatch) {
    let Some(meta) = &region.widget.edit else {
        return;
    };
    components.set_selected(Some(region.key.clone()));
    let text = components.edit_mut(&region.key, &meta.text).text.clone();
    write_text(region, &text, true, components);
    out.events.push(ScreenEvent::TextEditSelected {
        key: region.key.clone(),
        selected: true,
    });
}

fn deselect(region: &HitRegion, components: &mut Components, out: &mut Dispatch) {
    let Some(meta) = &region.widget.edit else {
        return;
    };
    if components.selected() == Some(region.key.as_str()) {
        components.set_selected(None);
    }
    let text = components.edit_mut(&region.key, &meta.text).text.clone();
    write_text(region, &text, false, components);
    out.events.push(ScreenEvent::TextEditSelected {
        key: region.key.clone(),
        selected: false,
    });
    out.events.push(text_event(region, &text, true));
}

/// The text target's text and selection, and the placeholder's visibility.
fn write_text(region: &HitRegion, text: &str, selected: bool, components: &mut Components) {
    let Some(meta) = &region.widget.edit else {
        return;
    };
    components.write(&region.key, "#text_edit_selected", Value::Bool(selected));
    if let Some((target, _)) = &meta.text_target {
        components.write(target, "text", Value::String(text.to_owned()));
        components.write(target, "#text_edit_selected", Value::Bool(selected));
        components.write(target, CARET_PROPERTY, Value::Bool(selected));
    }
    if let Some(placeholder) = &meta.placeholder {
        components.write(
            placeholder,
            "visible",
            Value::Bool(text.is_empty() && !selected),
        );
    }
}

/// The control's index in its named collection:
/// the enclosing instance's index unless it belongs to another collection.
fn collection_index(region: &HitRegion, collection: Option<&str>) -> Option<usize> {
    let Some(name) = collection else {
        return region.collection_index;
    };
    match region
        .collections
        .iter()
        .rev()
        .find(|(found, _)| found == name)
    {
        Some((_, index)) => Some(*index),
        None if region.collection.is_none() => region.collection_index,
        None => None,
    }
}

fn text_event(region: &HitRegion, text: &str, finished: bool) -> ScreenEvent {
    let meta = region.widget.edit.as_ref();
    ScreenEvent::TextEdit {
        name: meta.and_then(|meta| meta.name.clone()).unwrap_or_default(),
        key: region.key.clone(),
        index: collection_index(
            region,
            meta.and_then(|meta| meta.grid_collection.as_deref()),
        ),
        text: text.to_owned(),
        finished,
    }
}
