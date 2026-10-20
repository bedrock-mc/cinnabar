//! Bounded menu input and retained output on the existing serialized packet worker.

use super::Request;
use mod_api::native_menu::Input;
use mod_host::packet_mod::native_menu::Output;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

/// A validated retained panel and its static catalog, prepared outside the frame thread.
#[derive(Clone)]
pub struct MenuOutput {
    pub output: Output,
    pub prepared: Option<Arc<mod_ui::PreparedPanelUi>>,
}

/// The session owns this sender; the app reaches it weakly, without retaining the session.
pub(super) struct MenuLink {
    requests: Sender<Request>,
    pub(super) identity: u64,
    menu: Arc<NativeMenu>,
}

impl MenuLink {
    /// Attaches one mailbox to the packet worker's request channel.
    pub(super) fn new(requests: Sender<Request>, menu: Arc<NativeMenu>, identity: u64) -> Self {
        Self {
            requests,
            menu,
            identity,
        }
    }

    /// Whether the guest negotiated menus and the session still holds its grants.
    pub(super) fn available(&self) -> bool {
        self.menu.available()
    }

    /// Preserves ordered edits and key presses while coalescing the current visibility.
    pub(super) fn exchange(&self, input: Input) -> Option<Arc<MenuOutput>> {
        let (output, wake) = self.menu.exchange(input)?;
        if wake && self.requests.send(Request::NativeMenu).is_err() {
            self.menu.deactivate();
            return None;
        }
        output
    }

    /// Immediately releases pending input and output when the network session ends.
    pub(super) fn deactivate(&self) {
        self.menu.deactivate();
    }
}

#[derive(Default)]
struct State {
    epoch: u64,
    open: bool,
    pending: Option<Input>,
    /// Input phases preceding a visibility change, with their original presentation epoch.
    queued: VecDeque<MenuInput>,
    /// Combined queued and pending control/key work, independently bounded from phase count.
    work: usize,
    output: Option<(u64, Arc<MenuOutput>)>,
    /// Last panel delivered to the presenter, which declares incoming control kinds.
    input_panel: Option<Arc<MenuOutput>>,
    /// Earlier events belong to a previously delivered panel and cannot be folded.
    pointer_barrier: usize,
    wake_pending: bool,
}

/// A pending sample carries the visibility epoch that may accept its result.
pub(super) struct MenuInput {
    pub input: Input,
    epoch: u64,
}

/// A mailbox never owns a sender, so dropping the session also stops its worker.
pub(super) struct NativeMenu {
    state: Mutex<State>,
    ready: AtomicBool,
    stopped: AtomicBool,
    authority: Arc<AtomicBool>,
}

impl NativeMenu {
    /// Starts unavailable until session startup succeeds with an explicit controls grant.
    pub(super) fn new(authority: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            authority,
        }
    }

    /// Publishes negotiated support without reviving a stopped or replaced session.
    pub(super) fn activate(&self, ready: bool) {
        self.ready.store(
            ready && !self.stopped.load(Ordering::Acquire),
            Ordering::Release,
        );
    }

    /// Stops all menu effects synchronously, including results of an in-flight callback.
    pub(super) fn deactivate(&self) {
        self.ready.store(false, Ordering::Release);
        self.stopped.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending = None;
        state.queued.clear();
        state.work = 0;
        state.output = None;
        state.input_panel = None;
        state.pointer_barrier = 0;
        state.wake_pending = false;
        state.epoch = state.epoch.wrapping_add(1);
    }

    /// Current authority is checked at both publication and consumption boundaries.
    pub(super) fn available(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.stopped.load(Ordering::Acquire)
            && self.authority.load(Ordering::Acquire)
    }

    /// Returns each state change once and queues at most one worker wake for pending input.
    fn exchange(&self, input: Input) -> Option<(Option<Arc<MenuOutput>>, bool)> {
        if !self.available() || !input.is_valid() {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.available() {
            return None;
        }
        if input.open != state.open {
            if let Some(pending) = state.pending.take() {
                let epoch = state.epoch;
                state.queued.push_back(MenuInput {
                    input: pending,
                    epoch,
                });
            }
            state.pointer_barrier = 0;
            state.open = input.open;
            state.epoch = state.epoch.wrapping_add(1);
            state.output = state
                .output
                .take()
                .and_then(|(_, output)| hud_only(&output, state.open))
                .map(|output| (state.epoch, output));
        }
        let epoch = state.epoch;
        let output = state
            .output
            .take()
            .filter(|(generation, _)| *generation == epoch)
            .map(|(_, value)| value);
        let panel = state.input_panel.clone();
        let barrier = state.pointer_barrier;
        if let Some(panel) = panel
            .as_ref()
            .and_then(|output| output.output.panel.as_ref())
            && let Some(pending) = state.pending.as_mut()
        {
            let removed = coalesce_pointer_axes(panel, pending, &input, barrier);
            state.work -= removed;
        }
        let work = input.events.len() + input.keys.len();
        if state.work + work > mod_api::MAX_CONTROL_KEYS
            || state.queued.len() + 1 > mod_api::MAX_CONTROL_KEYS
        {
            drop(state);
            self.deactivate();
            return None;
        }
        state.work += work;
        if let Some(pending) = state.pending.as_mut() {
            pending.open = input.open;
            pending.pointer_held = input.pointer_held;
            pending.pointer_cancelled |= input.pointer_cancelled;
            pending.shift_held = input.shift_held;
            pending.viewport = input.viewport;
            pending.hud_geometry = input.hud_geometry;
            pending.preview_sizes = input.preview_sizes;
            pending.events.extend(input.events);
            pending.keys.extend(input.keys);
        } else {
            state.pending = Some(input);
        }
        if let Some(panel) = output
            .as_ref()
            .filter(|output| output.output.panel.is_some())
        {
            // This call's input came from the previously painted panel. Only
            // subsequent samples may use the newly delivered declaration.
            state.pointer_barrier = state.pending.as_ref().map_or(0, |input| input.events.len());
            state.input_panel = Some(Arc::clone(panel));
        }
        let wake = !std::mem::replace(&mut state.wake_pending, true);
        Some((output, wake))
    }

    /// Detaches one bounded ordered phase batch. New arrivals schedule one later worker wake.
    pub(super) fn take_inputs(&self) -> Vec<MenuInput> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.wake_pending = false;
        if !self.available() {
            return Vec::new();
        }
        let mut inputs: Vec<_> = state.queued.drain(..).collect();
        if let Some(input) = state.pending.take() {
            inputs.push(MenuInput {
                input,
                epoch: state.epoch,
            });
        }
        state.work = 0;
        state.pointer_barrier = 0;
        inputs
    }

    /// A close, grant change or stop invalidates an older in-flight presentation update.
    pub(super) fn complete(&self, input: &MenuInput, output: Option<Arc<MenuOutput>>) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.available()
            && let Some(output) = output
        {
            let output = if state.epoch == input.epoch {
                Some(output)
            } else {
                hud_only(&output, state.open)
            };
            if let Some(output) = output {
                state.output = Some((state.epoch, output));
            }
        }
    }
}

/// Keeps the latest held-pointer slider values within one uninterrupted action run.
/// Buttons, keys, cancellation, visibility and newly delivered panels remain barriers.
fn coalesce_pointer_axes(
    panel: &ui::mod_panel::Panel,
    pending: &mut Input,
    incoming: &Input,
    barrier: usize,
) -> usize {
    if !pending.open
        || !pending.pointer_held
        || !incoming.pointer_held
        || pending.pointer_cancelled
        || incoming.pointer_cancelled
        || !pending.keys.is_empty()
        || !incoming.keys.is_empty()
    {
        return 0;
    }
    let slider = |id: &str| {
        panel.controls.iter().any(|control| {
            matches!(control, ui::mod_panel::Control::Slider { id: declared, .. } if declared == id)
        })
    };
    let prefix = incoming
        .events
        .iter()
        .take_while(|event| slider(&event.id))
        .count();
    if prefix == 0 {
        return 0;
    }
    let start = pending.events[barrier..]
        .iter()
        .rposition(|event| !slider(&event.id))
        .map_or(barrier, |index| barrier + index + 1);
    let before = pending.events.len();
    let mut index = 0;
    pending.events.retain(|event| {
        let keep = index < start
            || !incoming.events[..prefix]
                .iter()
                .any(|next| next.id == event.id);
        index += 1;
        keep
    });
    before - pending.events.len()
}

/// Persistent HUD state is independent of visibility; stale menu controls never reopen it.
fn hud_only(output: &Arc<MenuOutput>, open: bool) -> Option<Arc<MenuOutput>> {
    if output.output.hud_bindings.is_none() && output.output.hud_geometry.is_none() {
        return None;
    }
    let mut retained = (**output).clone();
    retained.output.open = open;
    retained.output.panel = None;
    retained.prepared = None;
    Some(Arc::new(retained))
}

/// Retains a matching static catalog while bindings and control values change.
pub(super) fn prepare(
    mut output: Output,
    previous: &mut Option<Arc<MenuOutput>>,
) -> Result<Arc<MenuOutput>, String> {
    if let Some(previous) = previous.as_ref() {
        if output.panel.is_none() {
            output.panel.clone_from(&previous.output.panel);
        }
        if output.hud_bindings.is_none() {
            output
                .hud_bindings
                .clone_from(&previous.output.hud_bindings);
        }
        if output.input_extensions.is_none() {
            output
                .input_extensions
                .clone_from(&previous.output.input_extensions);
        }
        if output.hud_geometry.is_none() {
            output
                .hud_geometry
                .clone_from(&previous.output.hud_geometry);
        }
    }
    let prepared = match output.panel.as_ref() {
        Some(panel) => Some(
            match previous
                .as_ref()
                .and_then(|old| old.prepared.as_ref())
                .filter(|prepared| prepared.route(panel).is_some())
            {
                Some(prepared) => Arc::clone(prepared),
                None => mod_ui::PreparedPanelUi::prepare(Arc::new(ui::mod_panel::PanelBundle {
                    panel: panel.clone(),
                    surfaces: Default::default(),
                    control_schemas: Default::default(),
                }))?,
            },
        ),
        None => None,
    };
    let output = Arc::new(MenuOutput { output, prepared });
    *previous = Some(Arc::clone(&output));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::native_menu::{Event, Key};

    /// Makes a ready mailbox under revocable source authority.
    fn mailbox() -> (NativeMenu, Arc<AtomicBool>) {
        let authority = Arc::new(AtomicBool::new(true));
        let menu = NativeMenu::new(Arc::clone(&authority));
        menu.activate(true);
        (menu, authority)
    }

    /// Prepares a real native catalog without retaining an executable or asset fixture.
    fn output() -> Arc<MenuOutput> {
        let panel: ui::mod_panel::Panel = serde_json::from_value(serde_json::json!({
            "title":"Menu","toggle_key":"ShiftRight","dark":true,"controls":[],
            "surface":{"screen":"fixture.root","document":serde_json::json!({
                "namespace":"fixture","root":{"type":"panel","controls":[]}
            }).to_string()}
        }))
        .unwrap();
        prepare(
            Output {
                open: true,
                panel: Some(panel),
                hud_bindings: None,
                hud_geometry: None,
                input_extensions: None,
                keys: vec!["KeyR".into()],
            },
            &mut None,
        )
        .unwrap()
    }

    /// Advertises numeric drag axes and an ordered button through the real panel schema.
    fn drag_output() -> Arc<MenuOutput> {
        let mut declared = output().output.clone();
        declared.panel.as_mut().unwrap().controls = serde_json::from_value(serde_json::json!([
            {"id":"x","kind":"slider","label":"X","min":0,"max":1,"step":0.001,"value":0},
            {"id":"y","kind":"slider","label":"Y","min":0,"max":1,"step":0.001,"value":0},
            {"id":"apply","kind":"button","label":"Apply"}
        ]))
        .unwrap();
        prepare(declared, &mut None).unwrap()
    }

    /// Installs the same declaration that produces the subsequent pointer samples.
    fn drag_mailbox() -> NativeMenu {
        let (menu, _) = mailbox();
        menu.exchange(Input {
            open: true,
            ..Default::default()
        })
        .unwrap();
        let initial = menu.take_inputs().pop().unwrap();
        menu.complete(&initial, Some(drag_output()));
        assert!(
            menu.exchange(Input {
                open: true,
                ..Default::default()
            })
            .unwrap()
            .0
            .is_some()
        );
        menu.take_inputs();
        menu
    }

    #[test]
    fn sustained_pointer_axes_remain_available_when_the_worker_has_not_drained() {
        let menu = drag_mailbox();
        for index in 0..1000 {
            let input = Input {
                open: true,
                pointer_held: true,
                events: vec![
                    Event {
                        id: "x".into(),
                        value: index as f32 / 1000.0,
                    },
                    Event {
                        id: "y".into(),
                        value: 0.75,
                    },
                ],
                ..Default::default()
            };
            assert!(
                menu.exchange(input).is_some(),
                "valid drag sample {index} disabled the native menu"
            );
        }
        menu.exchange(Input {
            open: true,
            ..Default::default()
        })
        .unwrap();
        let inputs = menu.take_inputs();
        assert_eq!(inputs.len(), 1);
        assert!(!inputs[0].input.pointer_held);
        assert_eq!(
            inputs[0].input.events,
            vec![
                Event {
                    id: "x".into(),
                    value: 0.999
                },
                Event {
                    id: "y".into(),
                    value: 0.75
                }
            ]
        );
        menu.complete(&inputs[0], Some(drag_output()));
        assert!(
            menu.exchange(Input::default()).is_some(),
            "closing still reaches the guest"
        );
        assert!(
            menu.exchange(Input {
                open: true,
                ..Default::default()
            })
            .is_some(),
            "a later command can reopen the menu"
        );
        assert!(menu.available());
    }

    #[test]
    fn pointer_coalescing_keeps_button_key_cancellation_and_visibility_barriers() {
        let event = |id: &str, value| Event {
            id: id.into(),
            value,
        };
        let sample = |events| Input {
            open: true,
            pointer_held: true,
            events,
            ..Default::default()
        };
        let menu = drag_mailbox();
        menu.exchange(sample(vec![
            event("x", 0.1),
            event("apply", 1.0),
            event("x", 0.2),
            event("y", 0.3),
        ]))
        .unwrap();
        menu.exchange(sample(vec![event("x", 0.8), event("y", 0.9)]))
            .unwrap();
        let pending = menu.take_inputs().pop().unwrap();
        assert_eq!(
            pending.input.events,
            vec![
                event("x", 0.1),
                event("apply", 1.0),
                event("x", 0.8),
                event("y", 0.9)
            ]
        );
        for boundary in ["key", "cancel", "release", "visibility"] {
            let menu = drag_mailbox();
            let mut first = sample(vec![event("x", 0.1)]);
            match boundary {
                "key" => first.keys.push(Key {
                    key: "KeyR".into(),
                    text: None,
                }),
                "cancel" => first.pointer_cancelled = true,
                "release" => first.pointer_held = false,
                "visibility" => first.open = false,
                _ => unreachable!(),
            }
            menu.exchange(first).unwrap();
            menu.exchange(sample(vec![event("x", 0.9)])).unwrap();
            let phases = menu.take_inputs();
            assert_eq!(
                phases
                    .iter()
                    .flat_map(|phase| phase.input.events.iter())
                    .cloned()
                    .collect::<Vec<_>>(),
                vec![event("x", 0.1), event("x", 0.9)],
                "{boundary} was folded away"
            );
        }
    }

    #[test]
    fn a_new_panel_cannot_reclassify_a_pending_button_as_a_pointer_axis() {
        let menu = drag_mailbox();
        menu.exchange(Input {
            open: true,
            ..Default::default()
        })
        .unwrap();
        let request = menu.take_inputs().pop().unwrap();
        let mut buttons = drag_output().output.clone();
        buttons.panel.as_mut().unwrap().controls[0] = ui::mod_panel::Control::Button {
            id: "x".into(),
            label: "Action".into(),
        };
        menu.complete(&request, Some(prepare(buttons, &mut None).unwrap()));
        menu.exchange(Input {
            open: true,
            ..Default::default()
        })
        .unwrap();
        let request = menu.take_inputs().pop().unwrap();
        menu.complete(&request, Some(drag_output()));
        for value in [1.0, 0.2, 0.8] {
            menu.exchange(Input {
                open: true,
                pointer_held: true,
                events: vec![Event {
                    id: "x".into(),
                    value,
                }],
                ..Default::default()
            })
            .unwrap();
        }
        let request = menu.take_inputs().pop().unwrap();
        assert_eq!(
            request.input.events,
            vec![
                Event {
                    id: "x".into(),
                    value: 1.0
                },
                Event {
                    id: "x".into(),
                    value: 0.8
                }
            ]
        );
    }

    #[test]
    fn ordered_edits_survive_coalescing_without_queuing_multiple_wakes() {
        let (menu, _) = mailbox();
        let mut first = Input {
            viewport: Some([640.0, 400.0]),
            pointer_held: true,
            ..Default::default()
        };
        first.events.push(Event {
            id: "toggle".into(),
            value: 1.0,
        });
        first.hud_geometry.insert(
            "example.root".into(),
            mod_api::native_menu::HudGeometry {
                rect: [1.0, 2.0, 80.0, 27.0],
                size: [80.0, 27.0],
            },
        );
        assert!(menu.exchange(first).unwrap().1);
        let mut second = Input {
            viewport: Some([427.0, 240.0]),
            ..Default::default()
        };
        second.keys.push(Key {
            key: "KeyR".into(),
            text: None,
        });
        assert!(!menu.exchange(second).unwrap().1);
        let input = menu.take_inputs().pop().unwrap();
        assert_eq!(input.input.viewport, Some([427.0, 240.0]));
        assert!(
            input.input.hud_geometry.is_empty(),
            "hidden HUD roots clear older geometry"
        );
        assert!(
            !input.input.pointer_held,
            "the latest release is retained without an edit"
        );
        assert_eq!(input.input.events[0].id, "toggle");
        assert_eq!(input.input.keys[0].key, "KeyR");
        assert!(menu.take_inputs().is_empty());
    }

    #[test]
    fn pointer_cancellation_is_latched_until_consumed_while_shift_follows_the_latest_sample() {
        let (menu, _) = mailbox();
        menu.exchange(Input {
            pointer_cancelled: true,
            shift_held: true,
            ..Default::default()
        })
        .unwrap();
        menu.exchange(Input::default()).unwrap();
        let input = menu.take_inputs().pop().unwrap();
        assert!(
            input.input.pointer_cancelled,
            "a later idle frame cannot swallow cancellation"
        );
        assert!(!input.input.shift_held, "modifiers are a current snapshot");
        assert!(menu.take_inputs().is_empty());
        menu.exchange(Input {
            shift_held: true,
            ..Default::default()
        })
        .unwrap();
        let input = menu.take_inputs().pop().unwrap();
        assert!(
            !input.input.pointer_cancelled,
            "consumed cancellation is never replayed"
        );
        assert!(input.input.shift_held);
    }

    #[test]
    fn visibility_changes_and_revocation_discard_in_flight_presentations() {
        let (menu, authority) = mailbox();
        menu.exchange(Input::default()).unwrap();
        let old = menu.take_inputs().pop().unwrap();
        menu.exchange(Input {
            open: true,
            ..Default::default()
        })
        .unwrap();
        menu.complete(&old, Some(output()));
        assert!(
            menu.exchange(Input {
                open: true,
                ..Default::default()
            })
            .unwrap()
            .0
            .is_none()
        );
        let current = menu.take_inputs().pop().unwrap();
        menu.complete(&current, Some(output()));
        authority.store(false, Ordering::Release);
        assert!(
            menu.exchange(Input {
                open: true,
                ..Default::default()
            })
            .is_none()
        );
        menu.deactivate();
        menu.activate(true);
        assert!(!menu.available());
    }

    #[test]
    fn visibility_changes_preserve_ordered_control_actions_before_the_new_visibility_sample() {
        let (menu, _) = mailbox();
        menu.exchange(Input {
            events: vec![Event {
                id: "old".into(),
                value: 1.0,
            }],
            ..Default::default()
        })
        .unwrap();
        menu.exchange(Input {
            open: true,
            keys: vec![Key {
                key: "KeyR".into(),
                text: None,
            }],
            ..Default::default()
        })
        .unwrap();
        let inputs = menu.take_inputs();
        assert_eq!(inputs.len(), 2);
        assert!(!inputs[0].input.open);
        assert_eq!(inputs[0].input.events[0].id, "old");
        assert!(inputs[0].input.keys.is_empty());
        assert!(inputs[1].input.open);
        assert_eq!(inputs[1].input.keys[0].key, "KeyR");
        assert!(inputs[0].epoch < inputs[1].epoch);
    }

    #[test]
    fn visibility_phases_drain_in_one_bounded_batch_and_new_arrivals_rearm_one_wake() {
        let (menu, _) = mailbox();
        assert!(
            menu.exchange(Input {
                events: vec![Event {
                    id: "first".into(),
                    value: 1.0
                }],
                ..Default::default()
            })
            .unwrap()
            .1
        );
        assert!(
            !menu
                .exchange(Input {
                    open: true,
                    keys: vec![Key {
                        key: "KeyR".into(),
                        text: None
                    }],
                    ..Default::default()
                })
                .unwrap()
                .1
        );
        assert!(
            !menu
                .exchange(Input {
                    pointer_cancelled: true,
                    ..Default::default()
                })
                .unwrap()
                .1
        );
        let phases = menu.take_inputs();
        assert_eq!(
            phases.len(),
            3,
            "one wake detaches every phase already queued"
        );
        assert_eq!(
            phases
                .iter()
                .map(|phase| phase.input.open)
                .collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(phases[0].input.events[0].id, "first");
        assert_eq!(phases[1].input.keys[0].key, "KeyR");
        assert!(phases[2].input.pointer_cancelled);
        assert!(menu.take_inputs().is_empty());
        assert!(
            menu.exchange(Input::default()).unwrap().1,
            "new input during the detached batch schedules the next request"
        );
        assert!(!menu.exchange(Input::default()).unwrap().1);
        assert_eq!(menu.take_inputs().len(), 1);
    }

    #[test]
    fn queued_phases_keep_original_epochs_so_old_panel_results_cannot_reopen_the_menu() {
        let (menu, _) = mailbox();
        menu.exchange(Input {
            open: true,
            events: vec![Event {
                id: "edit".into(),
                value: 1.0,
            }],
            ..Default::default()
        })
        .unwrap();
        menu.exchange(Input::default()).unwrap();
        let phases = menu.take_inputs();
        assert_eq!(phases.len(), 2);
        menu.complete(&phases[0], Some(output()));
        assert!(
            menu.exchange(Input::default()).unwrap().0.is_none(),
            "the earlier visible phase cannot publish a stale panel"
        );
        let mut persistent = (*output()).clone();
        persistent.output.hud_bindings =
            Some(serde_json::from_value(serde_json::json!({"#mod_position":[0.2,0.4]})).unwrap());
        menu.complete(&phases[0], Some(Arc::new(persistent)));
        let update = menu.exchange(Input::default()).unwrap().0.unwrap();
        assert!(!update.output.open);
        assert!(update.output.panel.is_none() && update.prepared.is_none());
        assert!(update.output.hud_bindings.is_some());
    }

    #[test]
    fn queued_phase_and_combined_action_budgets_stop_and_clear_the_mailbox_on_overflow() {
        let (menu, _) = mailbox();
        let half = mod_api::MAX_CONTROL_KEYS / 2;
        menu.exchange(Input {
            events: vec![
                Event {
                    id: "edit".into(),
                    value: 1.0
                };
                half
            ],
            keys: vec![
                Key {
                    key: "KeyR".into(),
                    text: None
                };
                mod_api::MAX_CONTROL_KEYS - half
            ],
            ..Default::default()
        })
        .unwrap();
        assert!(
            menu.exchange(Input {
                open: true,
                events: vec![Event {
                    id: "extra".into(),
                    value: 1.0
                }],
                ..Default::default()
            })
            .is_none()
        );
        assert!(!menu.available());
        assert!(menu.take_inputs().is_empty());

        let (menu, _) = mailbox();
        for index in 0..mod_api::MAX_CONTROL_KEYS {
            assert!(
                menu.exchange(Input {
                    open: index % 2 == 1,
                    pointer_cancelled: true,
                    ..Default::default()
                })
                .is_some()
            );
        }
        assert!(
            menu.exchange(Input {
                open: mod_api::MAX_CONTROL_KEYS % 2 == 1,
                ..Default::default()
            })
            .is_none()
        );
        assert!(!menu.available());
        assert!(menu.take_inputs().is_empty());
    }

    #[test]
    fn deactivation_discards_every_queued_visibility_phase() {
        let (menu, _) = mailbox();
        menu.exchange(Input {
            pointer_cancelled: true,
            ..Default::default()
        })
        .unwrap();
        menu.exchange(Input {
            open: true,
            events: vec![Event {
                id: "edit".into(),
                value: 1.0,
            }],
            ..Default::default()
        })
        .unwrap();
        menu.deactivate();
        assert!(menu.take_inputs().is_empty());
        assert!(menu.exchange(Input::default()).is_none());
    }

    #[test]
    fn persistent_hud_updates_survive_visibility_changes_without_replaying_the_menu() {
        for close_before_completion in [false, true] {
            let (menu, _) = mailbox();
            menu.exchange(Input {
                open: true,
                ..Default::default()
            })
            .unwrap();
            let input = menu.take_inputs().pop().unwrap();
            let mut output = (*output()).clone();
            output.output.hud_bindings = Some(
                serde_json::from_value(serde_json::json!({
                    "#mod_fixture_position":[0.4,0.6]
                }))
                .unwrap(),
            );
            if close_before_completion {
                menu.exchange(Input::default()).unwrap();
            }
            menu.complete(&input, Some(Arc::new(output)));
            let update = menu.exchange(Input::default()).unwrap().0.unwrap();
            assert!(!update.output.open);
            assert!(update.output.panel.is_none() && update.prepared.is_none());
            assert!(update.output.hud_bindings.is_some());
        }
    }

    #[test]
    fn unchanged_static_panels_reuse_the_prepared_catalog() {
        let first = output();
        let mut previous = Some(Arc::clone(&first));
        let mut next = first.output.clone();
        next.open = false;
        let next = prepare(next, &mut previous).unwrap();
        assert!(Arc::ptr_eq(
            first.prepared.as_ref().unwrap(),
            next.prepared.as_ref().unwrap()
        ));
    }

    #[test]
    fn hud_only_updates_retain_an_existing_panel_without_preparing_a_catalog() {
        let globals = || {
            serde_json::from_value(serde_json::json!({
                "#mod_fixture_position":[0.25,0.75]
            }))
            .unwrap()
        };
        let mut previous = None;
        let first = prepare(
            Output {
                open: false,
                panel: None,
                hud_bindings: Some(globals()),
                hud_geometry: Some(ui::mod_hud::HudGeometryRequests(vec![
                    "example.root".into(),
                ])),
                input_extensions: None,
                keys: Vec::new(),
            },
            &mut previous,
        )
        .unwrap();
        assert!(first.prepared.is_none() && first.output.panel.is_none());

        let panel = output();
        let ready = prepare(panel.output.clone(), &mut previous).unwrap();
        assert_eq!(ready.output.hud_bindings, first.output.hud_bindings);
        assert_eq!(ready.output.hud_geometry, first.output.hud_geometry);
        let closed = prepare(
            Output {
                open: false,
                panel: None,
                hud_bindings: Some(globals()),
                hud_geometry: None,
                input_extensions: None,
                keys: Vec::new(),
            },
            &mut previous,
        )
        .unwrap();
        assert_eq!(closed.output.panel, ready.output.panel);
        assert!(Arc::ptr_eq(
            closed.prepared.as_ref().unwrap(),
            ready.prepared.as_ref().unwrap()
        ));
    }
}
