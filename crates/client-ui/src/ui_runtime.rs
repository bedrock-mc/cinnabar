//! Client presentation state and commands, with player authority borrowed from its owner.

pub mod bed;
pub mod book_screen;
mod boss;
pub mod chat_completion;
pub mod chat_send;
pub mod crafting_observation;
pub mod credits;
mod death;
pub mod emotes;
pub mod presentation_snapshot;
pub use inventory::CraftingPreview;
mod error;
pub mod event_apply;
pub use error::UiRuntimeError;
pub mod forms;
pub mod gameplay_authority;
pub mod gameplay_hud;
pub mod hud_adapter;
pub mod interaction;
pub mod inventory_actions;
pub mod inventory_drag;
pub mod inventory_ingress;
pub use inventory::inventory_ledger;
pub use inventory::inventory_router;
pub mod item_facts;
pub mod json_ui_assets;
pub mod oreui_assets;
pub mod oreui_fonts;
pub mod platform_clipboard;
pub mod presentation;
pub mod raw_text_resolution;
pub mod render_adapter;
pub mod scene_stack;
pub mod scoreboard_adapter;
pub mod screen_recipes;
pub mod screen_state;
pub mod session_data;
pub mod sign_editor;
pub mod use_on_identity_evidence;

pub use bed::SleepStatus;
pub use forms::{
    FormRespondError, FormTransportError, LocalFormAction, MAX_RETAINED_SERVER_FORMS,
    ServerFormEntry, ServerFormIdentity, ServerFormStore, flush_form_response,
};

pub use gameplay_authority::Translator;
pub use interaction::FastTransferAction;
#[cfg(any(test, feature = "test-support"))]
pub use interaction::dispatch_inventory_click;
pub use interaction::{ChatFlushError, flush_chat_sends, flush_inventory_send};
#[cfg(test)]
use interaction::{
    dispatch_chat_ui_action, gamepad_chat_action, paste_chat_shortcut,
    restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
};
pub use inventory_ingress::{InventoryAuthorityEvent, SequencedInventoryEvent};

use std::{collections::VecDeque, sync::Arc};

use bevy::prelude::Resource;
use protocol::{
    ActorAttribute, BlockCrackEvent, ChatAutocompleteCatalog, EquipmentEvent, InventoryAuthority,
    UiEvent,
};
use semantic_input::InputContext;
#[cfg(test)]
use ui::BoundedStat;
use ui::{
    BossBarStore, ChatAutocompleteRequest, ChatAutocompleteState, ChatClipboard, ChatEditor,
    ChatEditorError, ChatHistory, ChatPasteError, ChatRateLimit, ChatSendQueue, ChatStore,
    HudStore, MAX_CHAT_INPUT_BYTES, ScoreboardStore,
};

use self::gameplay_hud::GameplayHudState;
use self::inventory_ledger::PlayerInventoryLedger;
use self::inventory_router::{EquipmentRoute, InventoryRouterError};

pub use inventory::MAX_PENDING_INVENTORY_EVENTS;
const MAX_PENDING_CHAT_SENDS: usize = 32;
const MAX_CHAT_SENDS_PER_WINDOW: usize = 5;
const CHAT_RATE_WINDOW_MILLIS: u64 = 2_000;

pub use platform_clipboard::PlatformClipboard;

#[derive(Clone, Debug)]
pub struct SequencedUiEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    /// Ordering metadata only. Timed HUD state stamps exclusively from
    /// `local_millis`; a populated tick on a timed family is rejected by
    /// `apply` instead of being converted, so the two clocks can never mix.
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

pub use inventory::SequencedLocalEquipment;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiApplyOutcome {
    Applied,
    IgnoredByReceiveStore,
    // The typed document requires localization, scoreboard, or selector state that is not wired.
    IgnoredUnresolvedRawText,
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
    pub experiences: crate::experience_session::ExperienceSession,
    session_id: u64,
    last_fifo_sequence: Option<u64>,
    last_block_crack_sequence: Option<u64>,
    last_local_millis: Option<u64>,
    last_server_tick: Option<u64>,
    /// Presentation tick and real millis anchor, independent of packet ordering.
    presentation_tick_anchor: Option<(u64, u64)>,
    chat_focused: bool,
    inventory_open: bool,
    hud: HudStore,
    death_reason: Arc<str>,
    death_rules: protocol::DeathRules,
    toast_display_millis: u64,
    chat: ChatStore,
    scoreboards: ScoreboardStore,
    boss_bars: BossBarStore,
    boss_responses: boss::Responses,
    chat_editor: ChatEditor,
    chat_history: ChatHistory,
    chat_input_revision: u64,
    chat_autocomplete: ChatAutocompleteState,
    chat_autocomplete_catalog: ChatAutocompleteCatalog,
    chat_usage_hint: Option<Arc<str>>,
    local_sleeping: bool,
    wake_requested: bool,
    sleep_status: Option<bed::SleepStatus>,
    chat_tab_cycling: bool,
    chat_tab_start: Option<usize>,
    pending_chat_autocomplete_request: Option<ChatAutocompleteRequest>,
    chat_sends: ChatSendQueue,
    in_flight_chat_sends: usize,
    chat_source_name: Arc<str>,
    chat_xuid: Arc<str>,
    dropped_unsent_chat_messages: u64,
    block_cracks: crate::block_cracks::BlockCracks,
    gameplay_hud: GameplayHudState,
    use_on_identity_evidence: use_on_identity_evidence::UseOnIdentityEvidence,
    forms: ServerFormStore,
    credits: credits::CreditsState,
    sign_editor: sign_editor::SignEditor,
    inventory_pointer_gui: Option<[f32; 2]>,
    inventory_keys: interaction::InventoryKeys,
    screen: screen_state::ScreenState,
    furnace_projection: Arc<std::sync::Mutex<presentation::forms::furnace_book::ProjectionCache>>,
    emotes: emotes::EmoteState,
    /// Client packets the screens queue for the network flush.
    client_packets: VecDeque<protocol::Packet>,
    /// One complete book commit, bounded by the protocol page limit plus signing.
    book_packets: VecDeque<protocol::Packet>,
    local_actor_damage: Option<client_world::ActorDamageState>,
    /// Last known life state also retains the dead-to-alive reason retirement history.
    last_known_local_player_alive: Option<bool>,
    local_player_health_available: bool,
    last_selected_identity_change_millis: Option<u64>,
    last_selected_identity: Option<(u8, i32, u32)>,
    /// Local millis at which the held jump began charging the mounted jump
    /// bar; `None` while jump is released or no mount is ridden.
    mount_jump_hold_started_millis: Option<u64>,
    /// Startup-loaded localization catalog; survives session replacement
    /// because it is local pinned data, not server state.
    lang_catalog: Option<Arc<assets::RuntimeLangCatalog>>,
    active_lang: Option<Arc<assets::RuntimeLangCatalog>>,
    server_lang: Option<Arc<assets::ServerLangOverlay>>,
    session_icons: Option<Arc<presentation::SessionIcons>>,
    session_items: Option<Arc<item_facts::SessionItemComponents>>,
    server_ui: Option<Arc<presentation::ServerUiPack>>,
    session_glyphs: Option<Arc<presentation::SessionGlyphSheets>>,
    /// Authoritative display names of real player/entity score owners,
    /// refreshed from the world stream before committed events apply.
    score_owner_names: std::collections::BTreeMap<i64, Arc<str>>,
    /// Sorted usernames on the authoritative player list, the retained
    /// answer for the `@a` selector.
    known_player_names: Vec<Arc<str>>,
    player_list_held: bool,
    /// The live catalog's screen settings, which the scene stack reads.
    screen_settings: Arc<scene_stack::ScreenSettingsTable>,
    loading_screen: bool,
    hurt_pending: bool, // a health drop the scene stack has not yet answered
}

impl UiRuntime {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            experiences: Default::default(),
            last_fifo_sequence: None,
            last_block_crack_sequence: None,
            last_local_millis: None,
            last_server_tick: None,
            presentation_tick_anchor: None,
            chat_focused: false,
            inventory_open: false,
            score_owner_names: std::collections::BTreeMap::new(),
            known_player_names: Vec::new(),
            player_list_held: false,
            screen_settings: Arc::default(),
            loading_screen: false,
            hurt_pending: false,
            hud: HudStore::default(),
            death_reason: Arc::from(""),
            death_rules: protocol::DeathRules::default(),
            toast_display_millis: ui::TOAST_DISPLAY_MILLIS,
            chat: ChatStore::default(),
            scoreboards: ScoreboardStore::default(),
            boss_bars: BossBarStore::default(),
            boss_responses: boss::Responses::default(),
            chat_editor: ChatEditor::new(MAX_CHAT_INPUT_BYTES)
                .expect("the reviewed chat input bound is valid"),
            chat_history: ChatHistory::default(),
            chat_input_revision: 0,
            chat_autocomplete: ChatAutocompleteState::default(),
            chat_autocomplete_catalog: ChatAutocompleteCatalog::default(),
            chat_usage_hint: None,
            local_sleeping: false,
            wake_requested: false,
            sleep_status: None,
            chat_tab_cycling: false,
            chat_tab_start: None,
            pending_chat_autocomplete_request: None,
            chat_sends: ChatSendQueue::new(
                MAX_PENDING_CHAT_SENDS,
                ChatRateLimit::new(MAX_CHAT_SENDS_PER_WINDOW, CHAT_RATE_WINDOW_MILLIS)
                    .expect("the reviewed chat rate window is valid"),
            )
            .expect("the reviewed chat queue capacity is valid"),
            in_flight_chat_sends: 0,
            chat_source_name: Arc::from(""),
            chat_xuid: Arc::from(""),
            dropped_unsent_chat_messages: 0,
            block_cracks: crate::block_cracks::BlockCracks::default(),
            gameplay_hud: GameplayHudState::default(),
            use_on_identity_evidence:
                use_on_identity_evidence::UseOnIdentityEvidence::from_environment(session_id),
            forms: ServerFormStore::default(),
            credits: credits::CreditsState::default(),
            sign_editor: sign_editor::SignEditor::default(),
            inventory_pointer_gui: None,
            inventory_keys: interaction::InventoryKeys::default(),
            screen: screen_state::ScreenState::default(),
            emotes: emotes::EmoteState::default(),
            client_packets: VecDeque::new(),
            book_packets: VecDeque::new(),
            local_actor_damage: None,
            last_known_local_player_alive: None,
            local_player_health_available: false,
            last_selected_identity_change_millis: None,
            last_selected_identity: None,
            mount_jump_hold_started_millis: None,
            furnace_projection: Arc::default(),
            lang_catalog: None,
            active_lang: None,
            server_lang: None,
            session_icons: None,
            session_items: None,
            server_ui: None,
            session_glyphs: None,
        }
    }

    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    pub const fn inventory_authority(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> Option<InventoryAuthority> {
        player_runtime.inventory.inventory_authority()
    }

    pub const fn local_selected_equipment<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Option<&'a SequencedLocalEquipment> {
        player_runtime.inventory.local_selected_equipment()
    }

    pub fn publish_inventory_authority(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        authority: InventoryAuthority,
    ) {
        player_runtime
            .inventory
            .publish_inventory_authority(authority);
        if authority != InventoryAuthority::Server {
            self.inventory_open = false;
        }
    }

    /// The local player's StartGame-assigned runtime id, once known — required to address the
    /// local player in outbound packets such as the hotbar-selection `MobEquipment`.
    pub fn local_runtime_id(&self, player_runtime: &player_state::PlayerState) -> Option<u64> {
        player_runtime.inventory.local_runtime_id()
    }

    pub fn publish_local_runtime_id(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_id: u64,
        runtime_id: u64,
    ) -> Result<Vec<EquipmentRoute>, InventoryRouterError> {
        player_runtime
            .inventory
            .publish_local_runtime_id(session_id, runtime_id)
    }

    pub fn route_equipment(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_id: u64,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) -> Result<inventory_router::EquipmentRouteResult, InventoryRouterError> {
        player_runtime
            .inventory
            .route_equipment(session_id, fifo_sequence, event)
    }

    pub fn retain_local_selected_equipment(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        fifo_sequence: u64,
        event: EquipmentEvent,
    ) {
        if self.gameplay_hud.apply_offhand_equipment(&event) {
            return;
        }
        player_runtime
            .inventory
            .retain_local_selected_equipment(fifo_sequence, event);
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

    pub const fn inventory_open(&self) -> bool {
        self.inventory_open
    }

    pub const fn inventory_ledger<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> &'a PlayerInventoryLedger {
        player_runtime.inventory.ledger()
    }

    pub const fn sign_editor(&self) -> &sign_editor::SignEditor {
        &self.sign_editor
    }

    pub fn sign_editor_mut(&mut self) -> &mut sign_editor::SignEditor {
        &mut self.sign_editor
    }

    pub const fn server_forms(&self) -> &ServerFormStore {
        &self.forms
    }

    /// Answers one retained server form and stages its outbound
    /// `ModalFormResponse` for [`flush_form_response`].
    pub fn respond_to_server_form(
        &mut self,
        identity: ServerFormIdentity,
        action: LocalFormAction,
    ) -> Result<(), FormRespondError> {
        self.forms.respond(identity, action)
    }

    pub fn server_forms_mut(&mut self) -> &mut ServerFormStore {
        &mut self.forms
    }

    pub fn note_stream_dimension(&mut self, dimension: i32) {
        self.forms.note_stream_dimension(dimension);
        self.block_cracks.synchronize_dimension(Some(dimension));
    }

    pub fn inventory_ledger_mut<'a>(
        &mut self,
        player_runtime: &'a mut player_state::PlayerState,
    ) -> &'a mut PlayerInventoryLedger {
        player_runtime.inventory.ledger_mut()
    }

    pub fn poll_inventory_timeout(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        now_millis: u64,
    ) {
        if player_runtime
            .inventory
            .ledger_mut()
            .poll_timeout(now_millis)
        {
            self.inventory_open = player_runtime
                .inventory
                .ledger()
                .storage_generation()
                .is_some();
            self.inventory_pointer_gui = None;
        }
    }

    pub fn inventory_transport_closed(&mut self, player_runtime: &mut player_state::PlayerState) {
        player_runtime.inventory.ledger_mut().transport_closed();
        self.inventory_open = false;
        self.inventory_pointer_gui = None;
    }

    pub const fn inventory_pointer_gui(&self) -> Option<[f32; 2]> {
        self.inventory_pointer_gui
    }

    pub const fn screen_state(&self) -> &screen_state::ScreenState {
        &self.screen
    }

    pub fn screen_state_mut(&mut self) -> &mut screen_state::ScreenState {
        &mut self.screen
    }

    pub fn set_inventory_pointer_gui(&mut self, position: Option<[f32; 2]>) {
        self.inventory_pointer_gui = position;
    }

    /// Whether a scene over the game, menus aside, takes gameplay input.
    pub fn ui_focused(&self, player_runtime: &player_state::PlayerState) -> bool {
        !self.gameplay_input(player_runtime, None)
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

    pub fn chat_usage_hint(&self) -> Option<&str> {
        self.chat_usage_hint.as_deref()
    }

    pub fn take_chat_autocomplete_request(&mut self) -> Option<ChatAutocompleteRequest> {
        self.pending_chat_autocomplete_request.take()
    }

    pub fn service_pending_chat_autocomplete(
        &mut self,
        player_runtime: &player_state::PlayerState,
    ) -> bool {
        let Some(request) = self.take_chat_autocomplete_request() else {
            return false;
        };
        self.complete_chat_autocomplete(player_runtime, request)
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

    pub fn project_block_cracks(&mut self, snapshot: chunk_pipeline::BlockCrackSnapshot) {
        self.block_cracks.project(snapshot);
        self.block_cracks
            .report_status(self.session_id, self.block_cracks_status());
    }

    pub fn block_crack_snapshot(&self) -> Option<&chunk_pipeline::BlockCrackSnapshot> {
        self.block_cracks.snapshot()
    }

    pub fn block_cracks_status(&self) -> crate::block_cracks::BlockCrackStatus {
        self.block_cracks.status()
    }

    pub fn clear_disconnected_block_cracks(&mut self) {
        self.block_cracks.synchronize_dimension(None);
        self.block_cracks
            .report_status(self.session_id, self.block_cracks_status());
    }

    /// Resets UI session state for `session_id`; the session controller moves player authority with it.
    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id == session_id {
            return;
        }
        self.session_id = session_id;
        self.emotes.reset();
        self.client_packets.clear();
        self.book_packets.clear();
        self.screen = screen_state::ScreenState::default();
        self.experiences.reset();
        self.server_lang = None;
        self.session_icons = None;
        self.session_items = None;
        self.server_ui = None;
        self.session_glyphs = None;
        self.last_fifo_sequence = None;
        self.last_block_crack_sequence = None;
        self.last_local_millis = None;
        self.last_server_tick = None;
        self.presentation_tick_anchor = None;
        self.chat_source_name = Arc::from("");
        self.chat_xuid = Arc::from("");
        self.chat_focused = false;
        self.inventory_open = false;
        self.score_owner_names.clear();
        self.known_player_names.clear();
        self.player_list_held = false;
        self.hud.clear();
        self.death_reason = Arc::from("");
        self.death_rules = protocol::DeathRules::default();
        self.chat.clear();
        self.scoreboards.clear();
        self.boss_bars.clear();
        self.boss_responses = boss::Responses::default();
        self.chat_editor.clear();
        self.chat_history.clear_navigation();
        self.chat_input_revision = 0;
        self.chat_tab_cycling = false;
        self.chat_tab_start = None;
        self.chat_autocomplete.begin_session(session_id);
        self.chat_autocomplete_catalog = ChatAutocompleteCatalog::default();
        self.chat_usage_hint = None;
        self.local_sleeping = false;
        self.wake_requested = false;
        self.sleep_status = None;
        self.pending_chat_autocomplete_request = None;
        self.in_flight_chat_sends = 0;
        let dropped = self.chat_sends.begin_session(session_id);
        self.dropped_unsent_chat_messages = self
            .dropped_unsent_chat_messages
            .saturating_add(dropped as u64);
        self.block_cracks = crate::block_cracks::BlockCracks::default();
        self.sign_editor = sign_editor::SignEditor::default();
        self.gameplay_hud.clear();
        self.use_on_identity_evidence.reset(session_id);
        self.forms.clear();
        self.credits = credits::CreditsState::default();
        self.inventory_pointer_gui = None;
        self.local_actor_damage = None;
        self.last_known_local_player_alive = None;
        self.local_player_health_available = false;
        self.hurt_pending = false;
        self.last_selected_identity_change_millis = None;
        self.last_selected_identity = None;
        self.mount_jump_hold_started_millis = None;
    }

    pub fn open_chat(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> UiAuthorityTransition {
        player_runtime
            .inventory
            .ledger_mut()
            .request_storage_close();
        player_runtime
            .inventory
            .ledger_mut()
            .request_personal_close();
        self.inventory_open = false;
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
        self.chat_usage_hint = None;
        self.pending_chat_autocomplete_request = None;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    pub fn toggle_inventory(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> UiAuthorityTransition {
        self.chat_focused = false;
        if player_runtime
            .inventory
            .ledger()
            .storage_generation()
            .is_some()
        {
            player_runtime
                .inventory
                .ledger_mut()
                .request_storage_close();
            self.inventory_open = false;
        } else if self.inventory_open {
            player_runtime
                .inventory
                .ledger_mut()
                .request_personal_close();
            self.inventory_open = false;
        } else {
            self.inventory_open = self
                .local_runtime_id(player_runtime)
                .is_some_and(|runtime_id| {
                    player_runtime
                        .inventory
                        .ledger_mut()
                        .request_personal_open(runtime_id)
                });
        }
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: if self.inventory_open {
                InputContext::UiFocused
            } else {
                InputContext::Gameplay
            },
        }
    }

    pub fn close_inventory(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> UiAuthorityTransition {
        player_runtime
            .inventory
            .ledger_mut()
            .request_storage_close();
        player_runtime
            .inventory
            .ledger_mut()
            .request_personal_close();
        self.inventory_open = false;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    /// Applies an editor command and advances completion state only when its contents change.
    pub fn mutate_chat_editor(&mut self, mutate: impl FnOnce(&mut ChatEditor)) {
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
        self.chat_usage_hint = None;
        self.chat_tab_cycling = false;
        self.chat_tab_start = None;
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
        self.last_block_crack_sequence = Some(envelope.fifo_sequence);
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
}

#[cfg(test)]
pub mod tests;

impl UiRuntime {
    /// Borrows UI gesture state for the existing app input system.
    pub fn inventory_keys_mut(&mut self) -> &mut interaction::InventoryKeys {
        &mut self.inventory_keys
    }
    /// Applies the toast preference even when this frame cannot publish a viewport.
    pub fn set_toast_lifetime(&mut self, lifetime: u64) {
        self.toast_display_millis = lifetime;
    }

    /// Expires retained HUD output when the app has admitted a valid viewport.
    pub fn expire_hud(&mut self, now: u64) {
        self.hud.expire(now);
    }
}
