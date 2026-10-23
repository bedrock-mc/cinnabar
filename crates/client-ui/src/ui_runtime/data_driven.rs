//! Data-driven UI: the server's synced data stores and the one script screen they may show.

mod actions;
mod projection;

use std::collections::VecDeque;
use std::sync::Arc;

use oreui::data_driven_screen::DataDrivenForm;
use protocol::Packet;
use protocol::ddui::{
    DataDrivenScreenCloseReason, DataDrivenUiEvent, DataStores, ScriptedScreen, StoreApply,
    data_driven_screen_closed_packet,
};

pub use actions::{DataDrivenAction, FocusItems};

use super::scene_stack::SceneHost;
use super::{UiApplyOutcome, UiRuntime};

/// The script screen showing over the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveScreen {
    pub form_id: u32,
    pub screen: ScriptedScreen,
    /// The store property the screen reads.
    pub property: Arc<str>,
}

/// The session's DDUI data stores and its showing screen.
#[derive(Debug, Clone, Default)]
pub struct DataDrivenUi {
    stores: DataStores,
    active: Option<ActiveScreen>,
    /// The showing screen's control taking text or showing its options.
    focus: Option<actions::Focus>,
    /// A screen closed this frame: it keeps the frame's input, so screens updating after it
    /// cannot act on keys and clicks it already handled.
    closing: bool,
    ignored_store_changes: u64,
    /// Packets for the server, kept until the network flush takes them: every screen request
    /// waits for its answer, so none may be dropped when the shared queue is full.
    outgoing: VecDeque<Packet>,
    /// The host's screens (menus and the loading cover) as it last published them.
    host: SceneHost,
}

impl DataDrivenUi {
    /// The synced data stores the screens read.
    #[must_use]
    pub fn stores(&self) -> &DataStores {
        &self.stores
    }

    /// The screen showing over the world, if any.
    #[must_use]
    pub fn active(&self) -> Option<&ActiveScreen> {
        self.active.as_ref()
    }

    /// Whether a screen holds the player's input: while it shows, and for the rest of the frame
    /// it closes in.
    #[must_use]
    pub fn owns_input(&self) -> bool {
        self.active.is_some() || self.closing
    }

    /// Server store changes skipped because they named no known property, were written against
    /// another replacement of their property, or did not follow a path inside it.
    #[must_use]
    pub fn ignored_store_changes(&self) -> u64 {
        self.ignored_store_changes
    }

    pub(super) fn clear(&mut self) {
        self.stores.clear();
        self.active = None;
        self.focus = None;
        self.closing = false;
        self.outgoing.clear();
    }

    /// The oldest packet waiting for the network flush.
    pub(super) fn take_packet(&mut self) -> Option<Packet> {
        self.outgoing.pop_front()
    }
}

impl UiRuntime {
    /// Records the host's screens as this frame starts, which screen requests answered from UI
    /// events read along with the runtime's own screens.
    pub fn observe_host_screens(&mut self, host: SceneHost) {
        self.data_driven.host = host;
    }

    /// Whether the top screen shows a menu, so a screen request must answer busy: vanilla
    /// refuses one while a menu screen is showing.
    pub(super) fn data_driven_busy(&self, player_runtime: &player_state::PlayerState) -> bool {
        let host = SceneHost {
            loading: self.loading_screen,
            ..self.data_driven.host
        };
        self.shows_menu(player_runtime, host)
    }

    /// Applies a DDUI event. `busy` is true while a menu screen shows.
    ///
    /// Only one screen shows at a time: a request while busy or showing is answered `UserBusy`
    /// and the showing screen stays. A server close ends the matching screen (or any screen when
    /// it names none), and the client confirms it with the matching reason.
    pub(super) fn apply_data_driven_ui(
        &mut self,
        event: DataDrivenUiEvent,
        busy: bool,
    ) -> UiApplyOutcome {
        match event {
            DataDrivenUiEvent::Store(changes) => {
                for change in changes {
                    let outcome = self.data_driven.stores.apply(change);
                    if outcome != StoreApply::Applied {
                        self.data_driven.ignored_store_changes += 1;
                        tracing::debug!(target: "ddui", ?outcome, "data store change skipped");
                    }
                }
            }
            DataDrivenUiEvent::Show(show) => {
                let screen = ScriptedScreen::from_screen_id(&show.screen_id);
                let reason = if busy || self.data_driven.active.is_some() {
                    Some(DataDrivenScreenCloseReason::UserBusy)
                } else if screen.is_none() {
                    // Pack-defined DDUI screens are not drawn; closing them lets scripts go on.
                    Some(DataDrivenScreenCloseReason::ClientCanceled)
                } else {
                    None
                };
                tracing::info!(
                    target: "ddui",
                    screen = %show.screen_id,
                    form_id = show.form_id,
                    declined = ?reason,
                    "data-driven screen requested"
                );
                match (reason, screen) {
                    (Some(reason), _) => self.queue_screen_closed(show.form_id, reason),
                    (None, Some(screen)) => {
                        self.data_driven.focus = None;
                        self.data_driven.active = Some(ActiveScreen {
                            form_id: show.form_id,
                            screen,
                            property: screen.property(show.data_instance_id).into(),
                        });
                    }
                    (None, None) => {}
                }
            }
            DataDrivenUiEvent::Close { form_id } => {
                let matches = self
                    .data_driven
                    .active
                    .as_ref()
                    .is_some_and(|active| form_id.is_none_or(|id| id == active.form_id));
                if matches {
                    self.close_data_driven_screen(if form_id.is_some() {
                        DataDrivenScreenCloseReason::ProgrammaticClose
                    } else {
                        DataDrivenScreenCloseReason::ProgrammaticCloseAll
                    });
                }
            }
            // The client loads no pack DDUI definitions, so a reload changes nothing.
            DataDrivenUiEvent::Reload => {}
        }
        UiApplyOutcome::Applied
    }

    /// Ends the showing screen and reports `reason` to the server.
    pub fn close_data_driven_screen(&mut self, reason: DataDrivenScreenCloseReason) {
        self.data_driven.focus = None;
        if let Some(active) = self.data_driven.active.take() {
            self.data_driven.closing = true;
            self.queue_screen_closed(active.form_id, reason);
        }
    }

    /// Ends the frame a screen closed in, returning its input to the other screens; the host
    /// calls it as each frame starts.
    pub fn release_closed_data_driven_input(&mut self) {
        self.data_driven.closing = false;
    }

    /// The showing screen as OreUI draws it; `None` while nothing readable shows.
    #[must_use]
    pub fn data_driven_form(&self) -> Option<DataDrivenForm> {
        let active = self.data_driven.active.as_ref()?;
        self.project_screen(active)
            .map(|projection| projection.form)
    }

    fn queue_screen_closed(&mut self, form_id: u32, reason: DataDrivenScreenCloseReason) {
        self.data_driven
            .outgoing
            .push_back(data_driven_screen_closed_packet(form_id, reason));
    }
}
