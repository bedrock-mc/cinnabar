use semantic_input::identity::InputAuthorityGeneration;
use std::{collections::HashMap, num::NonZeroU64};

use bevy::prelude::{Res, ResMut, Resource};
use semantic_input::{
    ActionSnapshot, BindingError, ControlSettings, DeviceFrame, InputContext, KeyboardMouseFrame,
    ReleaseReason, RouterError, SemanticInputRouter, TouchContact,
};

pub(crate) mod physical;
pub(crate) use physical::{
    PendingDeviceFrame, SemanticRouteState, collect_raw_input,
    finalize_semantic_input_after_ui_authority, route_semantic_input,
};

use client_shell::ui_resources::UiRuntimeResource;
use {crate::runtime::world::ClientWorld, client_state::RuntimeSettings};

#[derive(Resource, Default)]
pub struct SemanticInputRuntime {
    router: SemanticInputRouter,
    previous: DeviceFrame,
    activity_sequence: u64,
    authority: Option<SemanticInputAuthorityIdentity>,
}

#[derive(Debug, Clone)]
pub struct SemanticInputAuthorityFrame {
    pub context: InputContext,
    pub controls_generation: u64,
    pub controls: ControlSettings,
    pub session_generation: NonZeroU64,
    pub dimension: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticInputAuthorityIdentity {
    context: InputContext,
    controls_generation: u64,
    session_generation: NonZeroU64,
    dimension: i32,
}

impl SemanticInputRuntime {
    pub fn route_and_finalize(
        &mut self,
        frame: DeviceFrame,
    ) -> Result<ActionSnapshot, RouterError> {
        self.route_device_frame(frame)?;
        self.finalize_routed_input()
    }

    pub(crate) fn route_device_frame(&mut self, mut frame: DeviceFrame) -> Result<(), RouterError> {
        if !frame.window_focus_lost {
            for previous in &self.previous.controllers {
                if !frame
                    .controllers
                    .iter()
                    .any(|current| current.device_id == previous.device_id)
                {
                    frame.disconnected_controllers.push(previous.device_id);
                }
            }
        }
        frame.disconnected_controllers.sort_unstable();
        frame.disconnected_controllers.dedup();
        frame
            .disconnected_controllers
            .truncate(semantic_input::MAX_DISCONNECTED_CONTROLLERS);
        self.stamp_activity(&mut frame);
        self.router.route(frame.clone())?;
        frame.desktop_events = None;
        // The router keeps these events; activity comparisons need only the held state.
        for controller in &mut frame.controllers {
            controller.button_edges = Default::default();
        }
        self.previous = frame;
        Ok(())
    }

    pub(crate) fn finalize_routed_input(&mut self) -> Result<ActionSnapshot, RouterError> {
        let snapshot = self.router.finalize()?;
        if self
            .previous
            .keyboard_mouse
            .as_ref()
            .is_some_and(|keyboard| !keyboard.keys.is_empty())
        {
            bevy::log::debug!(
                authority = ?self.authority,
                movement = ?snapshot.movement,
                input_mode = ?snapshot.input_mode,
                "finalized keyboard semantic input"
            );
        }
        Ok(snapshot)
    }

    pub fn set_context(&mut self, context: InputContext) {
        self.router.set_context(context);
    }

    pub fn replace_bindings(&mut self, settings: ControlSettings) -> Result<(), BindingError> {
        self.router.replace_bindings(settings)
    }

    pub fn replace_authority(&mut self, generation: InputAuthorityGeneration) {
        self.router.replace_authority(generation);
    }

    /// Consumes one UI-owned desktop control in the already sampled gameplay frame.
    pub(crate) fn consume_control(&mut self, control: semantic_input::PhysicalControl) {
        self.router.consume_control(control);
    }

    pub fn release_all(&mut self, reason: ReleaseReason) {
        bevy::log::info!("held inputs released: {reason:?}");
        self.router.release_all(reason);
    }

    pub fn synchronize_authority(
        &mut self,
        frame: SemanticInputAuthorityFrame,
    ) -> Result<bool, BindingError> {
        let identity = SemanticInputAuthorityIdentity {
            context: frame.context,
            controls_generation: frame.controls_generation,
            session_generation: frame.session_generation,
            dimension: frame.dimension,
        };
        let previous = self.authority;
        let changed = previous != Some(identity);
        if previous
            .is_none_or(|previous| previous.controls_generation != identity.controls_generation)
        {
            if previous.is_some() {
                bevy::log::info!(
                    "held inputs released: settings generation {} replaced the bindings",
                    identity.controls_generation
                );
            }
            self.replace_bindings(frame.controls)?;
        }
        if previous.is_none_or(|previous| previous.context != identity.context) {
            if let Some(previous) = previous {
                bevy::log::info!(
                    "held inputs released: input context {:?} -> {:?}",
                    previous.context,
                    identity.context
                );
            }
            self.set_context(identity.context);
        }
        if previous
            .is_none_or(|previous| previous.session_generation != identity.session_generation)
        {
            self.replace_authority(InputAuthorityGeneration::new(identity.session_generation));
        }
        if previous
            .is_some_and(|previous| previous.session_generation != identity.session_generation)
        {
            self.release_all(ReleaseReason::SessionReplaced);
        } else if previous.is_some_and(|previous| previous.dimension != identity.dimension) {
            self.release_all(ReleaseReason::DimensionReplaced);
        }
        self.authority = Some(identity);
        Ok(changed)
    }

    fn next_activity(&mut self) -> u64 {
        self.activity_sequence = self.activity_sequence.saturating_add(1);
        self.activity_sequence
    }

    fn stamp_activity(&mut self, frame: &mut DeviceFrame) {
        if let Some(current) = frame.keyboard_mouse.as_mut() {
            let previous = self.previous.keyboard_mouse.as_ref();
            current.activity_sequence = if current.mouse_motion.iter().all(|delta| *delta == 0.0)
                && previous.is_some_and(|previous| keyboard_physical_eq(previous, current))
            {
                previous.map_or(0, |previous| previous.activity_sequence)
            } else {
                self.next_activity()
            };
        }
        frame
            .controllers
            .sort_by_key(|controller| controller.device_id);
        let known_controller_activity_changed = frame.controllers.iter().any(|current| {
            !current.button_edges.pressed.is_empty() || !current.button_edges.released.is_empty()
        }) || {
            let previous = &self.previous.controllers;
            self.router.controller_activity_changed(
                previous.iter().filter(|previous| {
                    frame
                        .controllers
                        .iter()
                        .any(|current| current.device_id == previous.device_id)
                }),
                frame.controllers.iter().filter(|current| {
                    previous
                        .iter()
                        .any(|previous| previous.device_id == current.device_id)
                }),
            )
        };
        let known_controller_activity =
            known_controller_activity_changed.then(|| self.next_activity());
        for current in &mut frame.controllers {
            let previous = self
                .previous
                .controllers
                .iter()
                .find(|previous| previous.device_id == current.device_id);
            current.activity_sequence = match (previous, known_controller_activity) {
                (Some(_), Some(activity)) => activity,
                (Some(previous), None) => previous.activity_sequence,
                (None, _) => self.next_activity(),
            };
        }
        frame.touches.sort_by_key(|touch| touch.contact_id);
        for current in &mut frame.touches {
            let previous = self
                .previous
                .touches
                .iter()
                .find(|previous| previous.contact_id == current.contact_id);
            current.activity_sequence =
                if previous.is_some_and(|previous| touch_physical_eq(previous, current)) {
                    previous.map_or(0, |previous| previous.activity_sequence)
                } else {
                    self.next_activity()
                };
        }
    }
}

fn keyboard_physical_eq(left: &KeyboardMouseFrame, right: &KeyboardMouseFrame) -> bool {
    left.keys == right.keys
        && left.mouse_buttons == right.mouse_buttons
        && left.mouse_motion == right.mouse_motion
        && left.modifiers == right.modifiers
        && right.key_edges.pressed.is_empty()
        && right.key_edges.released.is_empty()
        && right.mouse_edges.pressed.is_empty()
        && right.mouse_edges.released.is_empty()
}

fn touch_physical_eq(left: &TouchContact, right: &TouchContact) -> bool {
    left.position == right.position && left.delta == right.delta && left.hit_id == right.hit_id
}

/// A contact's control and, for the stick and look surface, the delta it routes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SemanticTouchTarget {
    Control(u16, Option<[f32; 2]>),
}

/// This frame's touch control per held contact, contacts that lifted this frame, and the
/// controls a finger newly pressed this frame.
#[derive(Resource, Debug, Default)]
pub struct SemanticTouchTargets(
    HashMap<u64, SemanticTouchTarget>,
    Vec<TouchContact>,
    Vec<u16>,
);

impl SemanticTouchTargets {
    pub fn set(&mut self, contact_id: u64, hit_id: u16) {
        self.0
            .insert(contact_id, SemanticTouchTarget::Control(hit_id, None));
    }

    /// Targets a held contact with its sampled control and delta.
    pub(crate) fn set_sample(&mut self, sample: &TouchContact) {
        if let Some(hit_id) = sample.hit_id {
            self.0.insert(
                sample.contact_id,
                SemanticTouchTarget::Control(hit_id, Some(sample.delta)),
            );
        }
    }

    pub(crate) fn delta(&self, contact_id: u64) -> Option<[f32; 2]> {
        self.0
            .get(&contact_id)
            .and_then(|SemanticTouchTarget::Control(_, delta)| *delta)
    }

    /// Routes a contact that lifted this frame once, so a tap shorter than a frame still lands.
    pub(crate) fn pulse(&mut self, sample: TouchContact) {
        self.1.push(sample);
    }

    pub(crate) fn take_pulses(&mut self) -> Vec<TouchContact> {
        std::mem::take(&mut self.1)
    }

    /// Records a control a finger pressed this frame, for UI handlers keyed on press edges.
    pub(crate) fn press(&mut self, hit_id: u16) {
        self.2.push(hit_id);
    }

    /// Whether a finger pressed this control this frame, including a tap that already lifted.
    pub(crate) fn pressed(&self, hit_id: u16) -> bool {
        self.2.contains(&hit_id)
    }

    pub fn clear(&mut self, contact_id: u64) {
        self.0.remove(&contact_id);
    }

    pub fn retain_active_contacts(&mut self, contacts: impl IntoIterator<Item = u64>) {
        let mut contacts = contacts.into_iter().collect::<Vec<_>>();
        contacts.sort_unstable();
        contacts.dedup();
        self.0
            .retain(|contact_id, _| contacts.binary_search(contact_id).is_ok());
    }

    #[must_use]
    pub fn target(&self, contact_id: u64) -> Option<u16> {
        self.0
            .get(&contact_id)
            .map(|SemanticTouchTarget::Control(hit_id, _)| *hit_id)
    }

    pub fn release_all(&mut self) {
        self.0.clear();
        self.1.clear();
        self.2.clear();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(crate) fn synchronize_semantic_input_authority(
    mut runtime: ResMut<SemanticInputRuntime>,
    ui: Option<Res<UiRuntimeResource>>,
    focus: Res<client_state::ScreenFocus>,
    settings: Res<RuntimeSettings>,
    client_world: Option<Res<ClientWorld>>,
    mut touch_targets: ResMut<SemanticTouchTargets>,
) {
    let Some(ui) = ui.filter(|ui| ui.session_id() != 0) else {
        return;
    };
    let (controls_generation, controls) = settings.controls_update();
    let dimension = client_world
        .as_deref()
        .and_then(|world| world.stream.as_ref())
        .map_or(0, chunk_pipeline::WorldStream::current_dimension);
    let context = if focus.absorbs_input {
        InputContext::UiFocused
    } else {
        InputContext::Gameplay
    };
    let Some(session_generation) = NonZeroU64::new(ui.session_id()) else {
        return;
    };
    // An unchanged identity is a no-op; skip cloning the bindings for it every frame.
    if runtime.authority
        == Some(SemanticInputAuthorityIdentity {
            context,
            controls_generation,
            session_generation,
            dimension,
        })
    {
        return;
    }
    match runtime.synchronize_authority(SemanticInputAuthorityFrame {
        context,
        controls_generation,
        controls: controls.clone(),
        session_generation,
        dimension,
    }) {
        Ok(true) => touch_targets.release_all(),
        Ok(false) => {}
        Err(error) => {
            touch_targets.release_all();
            bevy::log::warn!(?error, "rejected semantic input authority replacement");
        }
    }
}

#[cfg(test)]
mod settings_tests;

#[cfg(test)]
mod tests {
    use super::SemanticInputRuntime;
    use semantic_input::{Action, ControllerFrame, DeviceFrame, InputMode, ReleaseReason};

    #[test]
    fn review_equal_nonzero_mouse_deltas_are_new_activity_each_frame() {
        let mut runtime = SemanticInputRuntime::default();
        let mut first = DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_motion: [1.0, 0.0],
                ..Default::default()
            }),
            ..Default::default()
        };
        runtime.stamp_activity(&mut first);
        runtime.previous = first.clone();
        let mut second = first;
        runtime.stamp_activity(&mut second);
        assert!(
            second.keyboard_mouse.unwrap().activity_sequence
                > runtime.previous.keyboard_mouse.unwrap().activity_sequence
        );
    }

    /// A completed controller tap cannot outrank fresh keyboard input on the next frame.
    #[test]
    fn consumed_controller_edges_do_not_steal_new_keyboard_movement() {
        let mut runtime = SemanticInputRuntime::default();
        let controller = ControllerFrame {
            device_id: 7,
            axes: [0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        };
        let tapped = runtime
            .route_and_finalize(DeviceFrame {
                keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
                controllers: vec![ControllerFrame {
                    button_edges: semantic_input::ButtonEdges {
                        pressed: vec![0],
                        released: vec![0],
                    },
                    ..controller.clone()
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(tapped.input_mode, InputMode::GamePad);
        assert_eq!(tapped.movement, [0.0, -1.0]);
        let jump = tapped.phases[Action::Jump as usize];
        assert!(jump.pressed && jump.released && !jump.held);
        let controller_activity = runtime.previous.controllers[0].activity_sequence;

        let keyboard = runtime
            .route_and_finalize(DeviceFrame {
                keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                    keys: vec![0x1a],
                    key_edges: semantic_input::ButtonEdges {
                        pressed: vec![0x1a],
                        released: vec![],
                    },
                    ..Default::default()
                }),
                controllers: vec![controller],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(keyboard.input_mode, InputMode::KeyboardMouse);
        assert_eq!(keyboard.movement, [0.0, 1.0]);
        assert_eq!(
            runtime.previous.controllers[0].activity_sequence,
            controller_activity
        );
    }

    #[test]
    fn cursor_unlock_does_not_synthesize_controller_disconnect() {
        let controller = ControllerFrame {
            device_id: 7,
            buttons: vec![0],
            ..ControllerFrame::default()
        };
        let mut runtime = SemanticInputRuntime::default();
        runtime
            .route_and_finalize(DeviceFrame {
                controllers: vec![controller.clone()],
                ..DeviceFrame::default()
            })
            .unwrap();

        let unlocked = runtime
            .route_and_finalize(DeviceFrame {
                keyboard_mouse: None,
                controllers: vec![controller],
                ..DeviceFrame::default()
            })
            .unwrap();
        assert!(unlocked.phases[Action::Jump as usize].held);
        assert!(!unlocked.phases[Action::Jump as usize].released);
        assert_eq!(unlocked.release_reasons[Action::Jump as usize], None);
    }

    #[test]
    fn focus_loss_is_not_misreported_as_controller_disconnect() {
        let mut runtime = SemanticInputRuntime::default();
        runtime
            .route_and_finalize(DeviceFrame {
                controllers: vec![ControllerFrame {
                    device_id: 7,
                    buttons: vec![0],
                    ..ControllerFrame::default()
                }],
                ..DeviceFrame::default()
            })
            .unwrap();

        let unfocused = runtime
            .route_and_finalize(DeviceFrame {
                window_focus_lost: true,
                ..DeviceFrame::default()
            })
            .unwrap();
        assert_eq!(
            unfocused.release_reasons[Action::Jump as usize],
            Some(ReleaseReason::WindowFocusLost)
        );
    }
}
