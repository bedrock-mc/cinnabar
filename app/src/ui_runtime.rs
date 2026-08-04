//! App-owned conversion boundary between retained UI output and render POD.

pub(crate) mod gameplay_touch;
mod hud_adapter;
mod interaction;
pub mod inventory_router;
mod items;
pub mod presentation;
pub mod render_adapter;
mod scoreboard_adapter;
mod titles;
mod translations;

pub use interaction::FastTransferAction;
pub use interaction::{ChatFlushError, flush_chat_sends};
#[cfg(test)]
use interaction::{
    dispatch_chat_ui_action, gamepad_chat_action, paste_chat_shortcut,
    restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
};
pub(crate) use interaction::{
    drive_chat_keyboard_input, drive_chat_ui_actions, flush_chat_network,
};

use std::{collections::BTreeMap, collections::VecDeque, sync::Arc};

use bevy::prelude::Resource;
use protocol::{
    ActorAttribute, BlockCrackEvent, ChatAutocompleteCatalog, ChatAutocompleteCatalogError,
    ChatPacketError, CommandOutputEvent, EquipmentEvent, HudEvent, InventoryAuthority,
    InventoryEvent, NetworkItemStack, Packet, PlayerGameMode, TextEvent, TextKind, TitleEvent,
    UiEvent, chat_input_packet,
};
use semantic_input::InputContext;
use ui::{
    BossBarStore, BoundedStat, ChatApplyResult, ChatAutocompleteError, ChatAutocompleteRequest,
    ChatAutocompleteResponse, ChatAutocompleteState, ChatClipboard, ChatEditor, ChatEditorError,
    ChatHistory, ChatMessage, ChatMessageKind, ChatPasteError, ChatRateLimit, ChatSendError,
    ChatSendQueue, ChatSendRequest, ChatStore, HudStore, MAX_CHAT_INPUT_BYTES,
    RetainedUiSequenceError, ScoreboardStore, Toast, UiAction,
};

use self::inventory_router::{EquipmentRoute, InventoryEquipmentRouter, InventoryRouterError};

pub const MAX_PENDING_BLOCK_CRACK_EVENTS: usize = 1_024;
pub const MAX_PENDING_INVENTORY_EVENTS: usize = 1_024;
const MAX_PENDING_CHAT_SENDS: usize = 32;
const MAX_CHAT_SENDS_PER_WINDOW: usize = 5;
const CHAT_RATE_WINDOW_MILLIS: u64 = 2_000;

#[derive(Default)]
pub(crate) struct PlatformClipboard;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformClipboardError {
    #[error("platform clipboard failed: {0}")]
    Platform(#[from] arboard::Error),
    #[error("clipboard text exceeds the {maximum}-byte chat insertion bound")]
    TooLong { maximum: usize },
}

impl ChatClipboard for PlatformClipboard {
    type Error = PlatformClipboardError;

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        let text = arboard::Clipboard::new()?.get_text()?;
        if text.len() > maximum_bytes {
            return Err(PlatformClipboardError::TooLong {
                maximum: maximum_bytes,
            });
        }
        Ok(Some(Arc::from(text)))
    }
}

#[derive(Clone, Debug)]
pub struct SequencedUiEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    pub server_tick: Option<u64>,
    pub event: UiEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencedBlockCrackEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub dimension: i32,
    pub event: BlockCrackEvent,
}

#[derive(Clone, Debug)]
pub struct SequencedLocalAttributes {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    pub server_tick: u64,
    pub attributes: Arc<[ActorAttribute]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedLocalEquipment {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub event: EquipmentEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInventoryEvent {
    pub session_generation: u64,
    pub fifo_sequence: u64,
    pub event: InventoryEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiApplyOutcome {
    Applied,
    IgnoredByReceiveStore,
    // The typed document requires localization, scoreboard, or selector state that is not wired.
    IgnoredUnresolvedRawText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRuntimeError {
    WrongSession { expected: u64, actual: u64 },
    StaleFifoSequence { previous: u64, actual: u64 },
    StaleBlockCrackSequence { previous: u64, actual: u64 },
    BlockCrackQueueFull { maximum: usize },
    InventoryQueueFull { maximum: usize },
    NonMonotonicLocalTime { previous: u64, actual: u64 },
    NonMonotonicServerTick { previous: u64, actual: u64 },
    InvalidTitleDurations,
    InvalidHealth(i32),
    InvalidLocalAttribute { field: &'static str },
    ChatRejected(ChatApplyResult),
    ChatAutocomplete(ChatAutocompleteError),
    ChatAutocompleteCatalog(ChatAutocompleteCatalogError),
    RetainedUiSequence(RetainedUiSequenceError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiAuthorityTransition {
    consumes_text: bool,
    requested_context: InputContext,
}

impl UiAuthorityTransition {
    pub const fn ui_consumed_text(self) -> bool {
        self.consumes_text
    }

    pub const fn requested_input_context(self) -> InputContext {
        self.requested_context
    }
}

#[derive(Clone, Debug, Resource)]
pub struct UiRuntime {
    session_id: u64,
    last_fifo_sequence: Option<u64>,
    last_block_crack_sequence: Option<u64>,
    last_local_millis: Option<u64>,
    last_server_tick: Option<u64>,
    chat_focused: bool,
    hud: HudStore,
    chat: ChatStore,
    scoreboards: ScoreboardStore,
    boss_bars: BossBarStore,
    chat_editor: ChatEditor,
    chat_history: ChatHistory,
    chat_input_revision: u64,
    chat_autocomplete: ChatAutocompleteState,
    chat_autocomplete_catalog: ChatAutocompleteCatalog,
    pending_chat_autocomplete_request: Option<ChatAutocompleteRequest>,
    chat_sends: ChatSendQueue,
    in_flight_chat_send: Option<(u64, u64)>,
    chat_source_name: Arc<str>,
    chat_xuid: Arc<str>,
    dropped_unsent_chat_messages: u64,
    pending_block_cracks: VecDeque<SequencedBlockCrackEvent>,
    inventory_authority: Option<InventoryAuthority>,
    player_game_mode: Option<PlayerGameMode>,
    last_inventory_sequence: Option<u64>,
    pending_inventory: VecDeque<SequencedInventoryEvent>,
    equipment_router: InventoryEquipmentRouter,
    local_selected_equipment: Option<SequencedLocalEquipment>,
    item_registry: BTreeMap<i32, Arc<str>>,
    hotbar: [NetworkItemStack; 9],
    base_translations: Arc<BTreeMap<Box<str>, Box<str>>>,
    translations: Arc<BTreeMap<Box<str>, Box<str>>>,
}

impl UiRuntime {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            last_fifo_sequence: None,
            last_block_crack_sequence: None,
            last_local_millis: None,
            last_server_tick: None,
            chat_focused: false,
            hud: HudStore::default(),
            chat: ChatStore::default(),
            scoreboards: ScoreboardStore::default(),
            boss_bars: BossBarStore::default(),
            chat_editor: ChatEditor::new(MAX_CHAT_INPUT_BYTES)
                .expect("the reviewed chat input bound is valid"),
            chat_history: ChatHistory::default(),
            chat_input_revision: 0,
            chat_autocomplete: ChatAutocompleteState::default(),
            chat_autocomplete_catalog: ChatAutocompleteCatalog::default(),
            pending_chat_autocomplete_request: None,
            chat_sends: ChatSendQueue::new(
                MAX_PENDING_CHAT_SENDS,
                ChatRateLimit::new(MAX_CHAT_SENDS_PER_WINDOW, CHAT_RATE_WINDOW_MILLIS)
                    .expect("the reviewed chat rate window is valid"),
            )
            .expect("the reviewed chat queue capacity is valid"),
            in_flight_chat_send: None,
            chat_source_name: Arc::from(""),
            chat_xuid: Arc::from(""),
            dropped_unsent_chat_messages: 0,
            pending_block_cracks: VecDeque::with_capacity(MAX_PENDING_BLOCK_CRACK_EVENTS),
            inventory_authority: None,
            player_game_mode: None,
            last_inventory_sequence: None,
            pending_inventory: VecDeque::with_capacity(MAX_PENDING_INVENTORY_EVENTS),
            equipment_router: InventoryEquipmentRouter::new(session_id),
            local_selected_equipment: None,
            item_registry: BTreeMap::new(),
            hotbar: std::array::from_fn(|_| NetworkItemStack::empty()),
            base_translations: Arc::new(BTreeMap::new()),
            translations: Arc::new(BTreeMap::new()),
        }
    }

    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    pub const fn inventory_authority(&self) -> Option<InventoryAuthority> {
        self.inventory_authority
    }

    pub const fn local_selected_equipment(&self) -> Option<&SequencedLocalEquipment> {
        self.local_selected_equipment.as_ref()
    }

    pub(crate) fn publish_inventory_authority(&mut self, authority: InventoryAuthority) {
        self.inventory_authority = Some(authority);
    }

    pub(crate) fn publish_player_game_mode(&mut self, game_mode: PlayerGameMode) {
        self.player_game_mode = Some(game_mode);
        if game_mode.shows_survival_stats() {
            self.hud.set_stats(
                BoundedStat::new(20, 20),
                BoundedStat::new(20, 20),
                self.hud.armor(),
                self.hud.air(),
            );
        } else {
            self.hud
                .set_stats(None, None, self.hud.armor(), self.hud.air());
        }
    }

    pub(crate) const fn survival_stats_visible(&self) -> bool {
        match self.player_game_mode {
            Some(game_mode) => game_mode.shows_survival_stats(),
            None => true,
        }
    }

    pub(crate) fn enqueue_inventory_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryEvent,
    ) -> Result<(), UiRuntimeError> {
        if session_generation != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: session_generation,
            });
        }
        if let Some(previous) = self.last_inventory_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if self.pending_inventory.len() >= MAX_PENDING_INVENTORY_EVENTS {
            return Err(UiRuntimeError::InventoryQueueFull {
                maximum: MAX_PENDING_INVENTORY_EVENTS,
            });
        }
        self.apply_inventory_visual_state(&event);
        self.pending_inventory.push_back(SequencedInventoryEvent {
            session_generation,
            fifo_sequence,
            event,
        });
        self.last_inventory_sequence = Some(fifo_sequence);
        Ok(())
    }

    pub fn pop_inventory_event(&mut self) -> Option<SequencedInventoryEvent> {
        self.pending_inventory.pop_front()
    }

    pub(crate) fn publish_local_runtime_id(
        &mut self,
        session_id: u64,
        runtime_id: u64,
    ) -> Result<Vec<EquipmentRoute>, InventoryRouterError> {
        self.equipment_router
            .publish_local_runtime_id(session_id, runtime_id)
    }

    pub(crate) fn route_equipment(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) -> Result<inventory_router::EquipmentRouteResult, InventoryRouterError> {
        self.equipment_router
            .route(session_id, fifo_sequence, event)
    }

    pub(crate) fn retain_local_selected_equipment(
        &mut self,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) {
        if event.window_id == 0
            && let Ok(slot) = usize::try_from(event.inventory_slot)
            && let Some(hotbar_slot) = self.hotbar.get_mut(slot)
        {
            *hotbar_slot = event.stack.clone();
        }
        self.local_selected_equipment = Some(SequencedLocalEquipment {
            session_id: self.session_id,
            fifo_sequence,
            event,
        });
    }

    pub const fn hud(&self) -> &HudStore {
        &self.hud
    }

    pub const fn chat(&self) -> &ChatStore {
        &self.chat
    }

    pub const fn scoreboards(&self) -> &ScoreboardStore {
        &self.scoreboards
    }

    pub const fn boss_bars(&self) -> &BossBarStore {
        &self.boss_bars
    }

    pub const fn chat_focused(&self) -> bool {
        self.chat_focused
    }

    pub const fn chat_editor(&self) -> &ChatEditor {
        &self.chat_editor
    }

    pub fn chat_suggestions(&self) -> &[Arc<str>] {
        self.chat_autocomplete.suggestions()
    }

    pub const fn chat_selected_suggestion(&self) -> Option<usize> {
        self.chat_autocomplete.selected_index()
    }

    pub fn take_chat_autocomplete_request(&mut self) -> Option<ChatAutocompleteRequest> {
        self.pending_chat_autocomplete_request.take()
    }

    pub fn complete_chat_autocomplete(&mut self, request: ChatAutocompleteRequest) -> bool {
        // Protocol 1001 UpdateSoftEnum packets are unsolicited catalog deltas and carry no
        // editor request identifier. Query the immutable catalog snapshot locally, then apply
        // the result only through the exact session/input/request correlation below.
        let Ok(completion) = self
            .chat_autocomplete_catalog
            .complete(&request.input, usize::from(request.cursor_byte))
        else {
            return false;
        };
        matches!(
            self.chat_autocomplete
                .apply_response(ChatAutocompleteResponse {
                    session: request.session,
                    input_revision: request.input_revision,
                    request_id: request.request_id,
                    catalog_revision: completion.catalog_revision,
                    suggestions: completion.suggestions,
                }),
            Ok(ui::ChatAutocompleteApply::Applied)
        )
    }

    pub fn service_pending_chat_autocomplete(&mut self) -> bool {
        let Some(request) = self.take_chat_autocomplete_request() else {
            return false;
        };
        self.complete_chat_autocomplete(request)
    }

    pub fn insert_chat_text(&mut self, value: &str) -> Result<(), ChatEditorError> {
        let before = self.chat_editor.clone();
        self.chat_editor.insert(value)?;
        if self.chat_editor != before {
            self.note_chat_editor_change();
        }
        Ok(())
    }

    pub fn paste_chat_text<C: ChatClipboard>(
        &mut self,
        clipboard: &mut C,
    ) -> Result<(), ChatPasteError<C::Error>> {
        let before = self.chat_editor.clone();
        self.chat_editor.paste_from(clipboard)?;
        if self.chat_editor != before {
            self.note_chat_editor_change();
        }
        Ok(())
    }

    pub fn move_chat_cursor_left(&mut self) {
        self.mutate_chat_editor(ChatEditor::move_left);
    }

    pub fn move_chat_cursor_right(&mut self) {
        self.mutate_chat_editor(ChatEditor::move_right);
    }

    pub fn backspace_chat_text(&mut self) {
        self.mutate_chat_editor(ChatEditor::backspace);
    }

    pub fn delete_chat_text(&mut self) {
        self.mutate_chat_editor(ChatEditor::delete);
    }

    pub fn move_chat_cursor_home(&mut self, selecting: bool) {
        self.mutate_chat_editor(|editor| editor.move_home(selecting));
    }

    pub fn move_chat_cursor_end(&mut self, selecting: bool) {
        self.mutate_chat_editor(|editor| editor.move_end(selecting));
    }

    pub fn show_older_chat_history(&mut self) -> bool {
        let Some(entry) = self.chat_history.older() else {
            return false;
        };
        self.replace_chat_editor(&entry);
        true
    }

    pub fn show_newer_chat_history(&mut self) -> bool {
        let Some(entry) = self.chat_history.newer() else {
            return false;
        };
        self.replace_chat_editor(&entry);
        true
    }

    pub fn handle_chat_ui_action(&mut self, action: UiAction) -> bool {
        let Some(suggestion) = self.chat_autocomplete.handle_action(action) else {
            return false;
        };
        self.replace_chat_editor(&suggestion);
        true
    }

    pub fn handle_chat_ui_action_with_suggestion_hit(
        &mut self,
        action: UiAction,
        suggestion_hit: Option<usize>,
    ) -> bool {
        if let UiAction::PointerPrimary {
            position: _,
            phase: ui::PointerPhase::Pressed,
        } = action
        {
            let Some(index) = suggestion_hit else {
                return false;
            };
            if !self.chat_autocomplete.select_index(index) {
                return false;
            }
            let Some(suggestion) = self.chat_autocomplete.selected_suggestion() else {
                return false;
            };
            self.replace_chat_editor(&suggestion);
            return true;
        }
        self.handle_chat_ui_action(action)
    }

    pub fn pending_chat_sends(&self) -> &VecDeque<ChatSendRequest> {
        self.chat_sends.pending()
    }

    pub const fn dropped_unsent_chat_messages(&self) -> u64 {
        self.dropped_unsent_chat_messages
    }

    pub fn set_chat_identity(&mut self, source_name: Arc<str>, xuid: Arc<str>) {
        self.chat_source_name = source_name;
        self.chat_xuid = xuid;
    }

    pub fn set_chat_source_name(&mut self, source_name: Arc<str>) {
        self.chat_source_name = source_name;
    }

    pub fn queue_chat_send(&mut self, now_millis: u64) -> Result<ChatSendRequest, ChatSendError> {
        let message = self.chat_editor.as_str();
        let request = self.chat_sends.push(self.session_id, message, now_millis)?;
        self.chat_history.push(Arc::clone(&request.message));
        self.chat_editor.clear();
        self.chat_autocomplete.clear();
        self.pending_chat_autocomplete_request = None;
        Ok(request)
    }

    pub fn front_chat_packet(&self) -> Result<Option<(u64, Packet)>, ChatPacketError> {
        self.chat_sends
            .pending()
            .front()
            .map(|request| {
                chat_input_packet(&self.chat_source_name, &self.chat_xuid, &request.message)
                    .map(|packet| (request.sequence, packet))
            })
            .transpose()
    }

    pub fn confirm_chat_send(&mut self, sequence: u64) -> bool {
        self.chat_sends.confirm_front(sequence)
    }

    pub const fn in_flight_chat_send(&self) -> Option<(u64, u64)> {
        self.in_flight_chat_send
    }

    pub fn mark_chat_send_enqueued(&mut self, session: u64, sequence: u64) -> bool {
        if self.in_flight_chat_send.is_some()
            || session != self.session_id
            || self
                .chat_sends
                .pending()
                .front()
                .is_none_or(|request| request.session != session || request.sequence != sequence)
        {
            return false;
        }
        self.in_flight_chat_send = Some((session, sequence));
        true
    }

    pub fn acknowledge_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if self.in_flight_chat_send != Some((session, sequence)) {
            return false;
        }
        self.in_flight_chat_send = None;
        self.confirm_chat_send(sequence)
    }

    pub fn fail_chat_send(&mut self, session: u64, sequence: u64) -> bool {
        if self.in_flight_chat_send != Some((session, sequence)) {
            return false;
        }
        self.in_flight_chat_send = None;
        true
    }

    pub const fn pending_block_cracks(&self) -> &VecDeque<SequencedBlockCrackEvent> {
        &self.pending_block_cracks
    }

    pub fn take_block_cracks(&mut self) -> Vec<SequencedBlockCrackEvent> {
        self.pending_block_cracks.drain(..).collect()
    }

    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id == session_id {
            return;
        }
        self.session_id = session_id;
        self.last_fifo_sequence = None;
        self.last_block_crack_sequence = None;
        self.last_local_millis = None;
        self.last_server_tick = None;
        self.chat_focused = false;
        self.hud.clear();
        self.chat.clear();
        self.scoreboards.clear();
        self.boss_bars.clear();
        self.chat_editor.clear();
        self.chat_history.clear_navigation();
        self.chat_input_revision = 0;
        self.chat_autocomplete.begin_session(session_id);
        self.chat_autocomplete_catalog = ChatAutocompleteCatalog::default();
        self.pending_chat_autocomplete_request = None;
        self.in_flight_chat_send = None;
        let dropped = self.chat_sends.begin_session(session_id);
        self.dropped_unsent_chat_messages = self
            .dropped_unsent_chat_messages
            .saturating_add(dropped as u64);
        self.pending_block_cracks.clear();
        self.inventory_authority = None;
        self.player_game_mode = None;
        self.last_inventory_sequence = None;
        self.pending_inventory.clear();
        self.equipment_router.begin_session(session_id);
        self.local_selected_equipment = None;
        self.item_registry.clear();
        self.hotbar = std::array::from_fn(|_| NetworkItemStack::empty());
        self.translations = Arc::clone(&self.base_translations);
    }

    pub(crate) fn install_base_translations(
        &mut self,
        translations: &BTreeMap<Box<str>, Box<str>>,
    ) {
        self.base_translations = Arc::new(translations.clone());
        self.translations = Arc::clone(&self.base_translations);
    }

    pub(crate) fn install_translations(&mut self, translations: &BTreeMap<Box<str>, Box<str>>) {
        let mut merged = (*self.base_translations).clone();
        merged.extend(
            translations
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        self.translations = Arc::new(merged);
    }

    pub fn open_chat(&mut self) -> UiAuthorityTransition {
        self.chat_focused = true;
        UiAuthorityTransition {
            consumes_text: true,
            requested_context: InputContext::UiFocused,
        }
    }

    pub fn close_chat(&mut self) -> UiAuthorityTransition {
        self.chat_focused = false;
        self.chat_editor.clear();
        self.chat_history.clear_navigation();
        self.chat_autocomplete.clear();
        self.pending_chat_autocomplete_request = None;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    pub fn apply(&mut self, envelope: SequencedUiEvent) -> Result<UiApplyOutcome, UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            envelope.server_tick,
        )?;
        let event_millis = envelope
            .server_tick
            .map_or(envelope.local_millis, |tick| tick.saturating_mul(50));
        let outcome = match envelope.event {
            UiEvent::Text(event) => self.apply_text(event, envelope.fifo_sequence, event_millis)?,
            UiEvent::CommandOutput(event) => {
                self.apply_command_output(event, envelope.fifo_sequence, event_millis)?
            }
            UiEvent::RawText(mut event) if event.document.has_unresolved_components() => {
                match translations::resolve_raw_text(&self.translations, &event.document) {
                    Some(message) => {
                        event.text.message = message;
                        self.apply_text(event.text, envelope.fifo_sequence, event_millis)?
                    }
                    None => UiApplyOutcome::IgnoredUnresolvedRawText,
                }
            }
            UiEvent::RawText(event) => {
                self.apply_text(event.text, envelope.fifo_sequence, event_millis)?
            }
            UiEvent::Title(event)
                if event
                    .document
                    .as_ref()
                    .is_some_and(|document| document.has_unresolved_components()) =>
            {
                match event.document.as_ref().and_then(|document| {
                    translations::resolve_raw_text(&self.translations, document)
                }) {
                    Some(message) => {
                        titles::apply_title(
                            self,
                            TitleEvent {
                                text: message,
                                document: None,
                                ..event
                            },
                            envelope.fifo_sequence,
                            event_millis,
                        )?;
                        UiApplyOutcome::Applied
                    }
                    None => UiApplyOutcome::IgnoredUnresolvedRawText,
                }
            }
            UiEvent::Title(event) => {
                titles::apply_title(self, event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Hud(event) => {
                self.apply_hud(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::ChatAutocomplete(event) => {
                self.chat_autocomplete_catalog
                    .apply(event)
                    .map_err(UiRuntimeError::ChatAutocompleteCatalog)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Objective(event) => scoreboard_adapter::apply_outcome(
                self.scoreboards
                    .apply(envelope.fifo_sequence, scoreboard_adapter::objective(event))
                    .map_err(UiRuntimeError::RetainedUiSequence)?,
            ),
            UiEvent::Score(event) => scoreboard_adapter::apply_outcome(
                self.scoreboards
                    .apply(envelope.fifo_sequence, scoreboard_adapter::score(event))
                    .map_err(UiRuntimeError::RetainedUiSequence)?,
            ),
            UiEvent::Boss(event) => scoreboard_adapter::apply_outcome(
                self.boss_bars
                    .apply(envelope.fifo_sequence, scoreboard_adapter::boss(event))
                    .map_err(UiRuntimeError::RetainedUiSequence)?,
            ),
            UiEvent::Form(_) => UiApplyOutcome::IgnoredByReceiveStore,
        };
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        if let Some(server_tick) = envelope.server_tick {
            self.last_server_tick = Some(server_tick);
        }
        Ok(outcome)
    }

    fn mutate_chat_editor(&mut self, mutate: impl FnOnce(&mut ChatEditor)) {
        let before = self.chat_editor.clone();
        mutate(&mut self.chat_editor);
        if self.chat_editor != before {
            self.note_chat_editor_change();
        }
    }

    fn replace_chat_editor(&mut self, value: &str) {
        if self.chat_editor.as_str() == value
            && self.chat_editor.cursor_byte() == self.chat_editor.len_bytes()
        {
            return;
        }
        self.chat_editor.clear();
        self.chat_editor
            .insert(value)
            .expect("history and autocomplete entries obey the chat input bound");
        self.note_chat_editor_change();
    }

    fn note_chat_editor_change(&mut self) {
        self.chat_input_revision = self.chat_input_revision.saturating_add(1);
        self.pending_chat_autocomplete_request = self
            .chat_autocomplete
            .begin_input(
                self.session_id,
                self.chat_input_revision,
                self.chat_editor.as_str(),
                self.chat_editor.cursor_byte(),
            )
            .expect("the editor enforces autocomplete input and cursor bounds");
    }

    pub fn retain_block_crack(
        &mut self,
        envelope: SequencedBlockCrackEvent,
    ) -> Result<(), UiRuntimeError> {
        if envelope.session_id != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: envelope.session_id,
            });
        }
        if let Some(previous) = self.last_block_crack_sequence
            && envelope.fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleBlockCrackSequence {
                previous,
                actual: envelope.fifo_sequence,
            });
        }
        if self.pending_block_cracks.len() >= MAX_PENDING_BLOCK_CRACK_EVENTS {
            return Err(UiRuntimeError::BlockCrackQueueFull {
                maximum: MAX_PENDING_BLOCK_CRACK_EVENTS,
            });
        }
        self.last_block_crack_sequence = Some(envelope.fifo_sequence);
        self.pending_block_cracks.push_back(envelope);
        Ok(())
    }

    pub fn apply_local_attributes(
        &mut self,
        envelope: SequencedLocalAttributes,
    ) -> Result<(), UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            Some(envelope.server_tick),
        )?;
        let mut health = self.hud.health();
        let mut hunger = self.hud.hunger();
        for attribute in envelope.attributes.iter() {
            match attribute.name.as_ref() {
                "minecraft:health" => {
                    health = Some(
                        hud_adapter::attribute_stat(attribute)
                            .ok_or(UiRuntimeError::InvalidLocalAttribute { field: "health" })?,
                    );
                }
                "minecraft:player.hunger" => {
                    hunger = Some(
                        hud_adapter::attribute_stat(attribute)
                            .ok_or(UiRuntimeError::InvalidLocalAttribute { field: "hunger" })?,
                    );
                }
                _ => {}
            }
        }
        self.hud
            .set_stats(health, hunger, self.hud.armor(), self.hud.air());
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        self.last_server_tick = Some(envelope.server_tick);
        Ok(())
    }

    fn validate_identity(
        &self,
        session_id: u64,
        fifo_sequence: u64,
        local_millis: u64,
        server_tick: Option<u64>,
    ) -> Result<(), UiRuntimeError> {
        if session_id != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: session_id,
            });
        }
        if let Some(previous) = self.last_fifo_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if let Some(previous) = self.last_local_millis
            && local_millis < previous
        {
            return Err(UiRuntimeError::NonMonotonicLocalTime {
                previous,
                actual: local_millis,
            });
        }
        if let (Some(previous), Some(actual)) = (self.last_server_tick, server_tick)
            && actual < previous
        {
            return Err(UiRuntimeError::NonMonotonicServerTick { previous, actual });
        }
        Ok(())
    }

    fn apply_text(
        &mut self,
        event: TextEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        let should_translate =
            event.needs_translation || matches!(event.kind, TextKind::Translation);
        let message = translations::resolve_translation(
            &self.translations,
            event.message,
            event.parameters.as_ref(),
            should_translate,
        );
        if matches!(
            event.kind,
            TextKind::Popup | TextKind::JukeboxPopup | TextKind::Tip
        ) {
            self.hud.set_actionbar(message, fifo_sequence, event_millis);
            return Ok(UiApplyOutcome::Applied);
        }
        let kind = match event.kind {
            TextKind::Chat => ChatMessageKind::Chat,
            TextKind::Whisper | TextKind::JsonWhisper => ChatMessageKind::Whisper,
            TextKind::Announcement | TextKind::JsonAnnouncement => ChatMessageKind::Announcement,
            TextKind::Translation => ChatMessageKind::Translation,
            TextKind::Raw | TextKind::System | TextKind::Json => ChatMessageKind::System,
            TextKind::Popup | TextKind::JukeboxPopup | TextKind::Tip => unreachable!(),
        };
        match self.chat.push(ChatMessage {
            fifo_sequence,
            received_millis: event_millis,
            kind,
            source: event.source,
            message,
            parameters: Arc::from([]),
        }) {
            ChatApplyResult::Applied { .. } => Ok(UiApplyOutcome::Applied),
            result => Err(UiRuntimeError::ChatRejected(result)),
        }
    }

    fn apply_command_output(
        &mut self,
        event: CommandOutputEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        let messages = event
            .messages
            .iter()
            .map(|message| ChatMessage {
                fifo_sequence,
                received_millis: event_millis,
                kind: ChatMessageKind::Translation,
                source: None,
                message: translations::resolve_translation(
                    &self.translations,
                    Arc::clone(&message.message_id),
                    message.parameters.as_ref(),
                    true,
                ),
                parameters: Arc::from([]),
            })
            .collect();
        match self.chat.push_batch(messages) {
            ChatApplyResult::Applied { .. } => Ok(UiApplyOutcome::Applied),
            result => Err(UiRuntimeError::ChatRejected(result)),
        }
    }

    fn apply_hud(
        &mut self,
        event: HudEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        match event {
            HudEvent::Toast { title, message } => {
                self.hud.push_toast(Toast {
                    title,
                    message,
                    fifo_sequence,
                    received_millis: event_millis,
                });
            }
            HudEvent::Health { health } => {
                let health =
                    u16::try_from(health).map_err(|_| UiRuntimeError::InvalidHealth(health))?;
                let maximum = health.max(20);
                self.hud.set_health(BoundedStat::new(health, maximum));
            }
            HudEvent::PlayerStatus(status) => {
                self.hud
                    .set_player_status(hud_adapter::player_status(status));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
