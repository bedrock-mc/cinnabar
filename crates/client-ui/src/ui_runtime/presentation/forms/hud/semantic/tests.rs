//! Semantic equivalence and construction bounds without installed game assets.

use super::*;
use crate::ui_runtime::presentation::retained_hud::{
    PresentedScoreboardCache, ScoreboardOwnerNameAuthority,
};
use json_ui::{HudBindings, hud_data_source};
use ui::{BoundedStat, ChatMessage, ChatMessageKind};

struct Fixture {
    player: player_state::PlayerState,
    runtime: UiRuntime,
    frame: HudFrame,
    settings: SettingsOptions,
    catalog: Arc<Catalog>,
    scoreboard: PresentedScoreboardCache,
    names: ScoreboardOwnerNameAuthority,
    hide_sidebar: bool,
    hide_effects: bool,
    bindings: HudBindings,
}

impl Default for Fixture {
    /// Makes a survival HUD with no local carrier or external resources.
    fn default() -> Self {
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        player.inventory.set_local_selected_slot(0);
        Self {
            player,
            runtime: UiRuntime::new(1),
            frame: HudFrame::default(),
            settings: SettingsOptions::default(),
            catalog: Arc::new(Catalog::default()),
            scoreboard: Default::default(),
            names: Default::default(),
            hide_sidebar: false,
            hide_effects: false,
            bindings: Default::default(),
        }
    }
}

impl Fixture {
    /// Checks the retained model, icon references and bindings against the uncached projection.
    fn check(&mut self, cache: &mut SemanticHud) -> HudChanges {
        let key = (
            self.runtime.scoreboards().revision(),
            self.names.revision(),
            self.hide_sidebar,
        );
        let sidebar = self
            .scoreboard
            .refresh(self.runtime.scoreboards(), &self.names)
            .filter(|_| !self.hide_sidebar);
        let changes = cache.refresh(
            &self.player,
            &self.runtime,
            &self.frame,
            sidebar,
            key,
            &self.settings,
            self.hide_effects,
            &self.catalog,
        );
        let mut icons = Vec::new();
        let mut expected = super::super::reference::hud_model(
            &self.player,
            &self.runtime,
            &self.frame,
            sidebar.map(super::super::reference::sidebar_model),
            &mut icons,
            &self.settings,
        );
        if self.hide_effects {
            expected.effects_visible = false;
        }
        ui_screens::settings_chat::apply_hud(&self.settings, &mut expected);
        assert_eq!(cache.model, expected);
        assert_eq!(cache.icons, icons);
        assert_eq!(
            *self.bindings.update(&cache.model, changes),
            hud_data_source(&expected)
        );
        changes
    }

    /// Appends an independently identified local chat row, including future timestamps.
    fn chat(&mut self, text: &str, received_millis: u64) {
        self.runtime.chat.push_local(ChatMessage {
            fifo_sequence: 0,
            received_millis,
            kind: ChatMessageKind::System,
            source: None,
            message: Arc::from(text),
            parameters: Arc::from([]),
        });
    }
}

/// A captured stack whose count, icon and durability can change independently.
fn stack(count: u16) -> protocol::NetworkItemStack {
    protocol::NetworkItemStack {
        network_id: 1,
        metadata: 0,
        stack_network_id: 1,
        count,
        nbt_digest: [0; 32],
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

#[test]
fn idle_and_live_paint_inputs_do_not_reconstruct_semantic_components() {
    let mut fixture = Fixture::default();
    fixture.chat("§kMoving glyphs", 0);
    fixture.frame.hotbar_stacks[0] = Some(stack(4));
    fixture.frame.hotbar_icons[0] = Some(IconRef {
        page: 2,
        uv: [0, 0, 16, 16],
        glint: false,
    });
    fixture.runtime.hud.set_title(Arc::from("Title"), 1, 0);
    let mut cache = SemanticHud::default();
    assert_eq!(fixture.check(&mut cache), HudChanges::ALL);
    let before = cache.constructions;
    let initial_paint = crate::ui_runtime::presentation::hud_layout::capture_hud_paint(
        &fixture.player,
        &fixture.runtime,
        &fixture.frame,
        None,
        &fixture.settings,
    );
    let text = cache.model.chat[0].text.as_ptr();
    let hotbar = cache.model.hotbar.as_ptr();
    let icons = cache.icons.as_ptr();
    for now in [1, 100, 300] {
        fixture.frame.now_millis = now;
        fixture.frame.hotbar_cooldowns[0] = now as f32 / 500.0;
        fixture.frame.attack_indicator_charge = Some(0.5);
        fixture.frame.mount_jump = Some(0.4);
        fixture
            .runtime
            .hud
            .set_stats(BoundedStat::new(3, 20), BoundedStat::new(2, 20), None, None);
        assert_eq!(fixture.check(&mut cache), HudChanges::default());
        assert_eq!(cache.constructions, before);
        assert_eq!(cache.model.chat[0].text.as_ptr(), text);
        assert_eq!(cache.model.hotbar.as_ptr(), hotbar);
        assert_eq!(cache.icons.as_ptr(), icons);
        assert_eq!(cache.model.title.as_ref().unwrap().born, 0.0);
        let paint = crate::ui_runtime::presentation::hud_layout::capture_hud_paint(
            &fixture.player,
            &fixture.runtime,
            &fixture.frame,
            None,
            &fixture.settings,
        );
        assert_ne!(paint, initial_paint);
        assert_eq!(paint.hearts.current, 3);
        assert_eq!(paint.hotbar_cooldowns, fixture.frame.hotbar_cooldowns);
    }
}

#[test]
fn hotbar_corrections_and_artwork_keep_icons_and_other_components_independent() {
    let mut fixture = Fixture::default();
    fixture.chat("Retained", 0);
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    let chat = cache.model.chat[0].text.as_ptr();
    for (slot, count, page, durability) in [
        (0, 64, 2, Some(0.8)),
        (1, 2, 3, None),
        (0, 1, 4, Some(0.2)),
        (1, 0, 5, None),
    ] {
        fixture.frame.hotbar_stacks[slot] = (count != 0).then(|| stack(count));
        fixture.frame.hotbar_icons[slot] = Some(IconRef {
            page,
            uv: [0, 0, 16, 16],
            glint: false,
        });
        fixture.frame.hotbar_durability[slot] = durability;
        fixture.player.inventory.set_local_selected_slot(slot as u8);
        let changes = fixture.check(&mut cache);
        assert!(changes.hotbar);
        assert!(!changes.chat && !changes.sidebar && !changes.boss_bars);
        assert_eq!(cache.model.chat[0].text.as_ptr(), chat);
    }
    fixture.frame.hotbar_cooldowns[0] = 0.1;
    assert_eq!(fixture.check(&mut cache), HudChanges::default());
}

#[test]
fn timed_text_preserves_requests_and_reacts_to_expiry_and_time_jumps() {
    let mut fixture = Fixture::default();
    fixture.runtime.hud.set_durations(ui::TitleDurations {
        fade_in_ticks: 2,
        stay_ticks: 2,
        fade_out_ticks: 2,
    });
    fixture.runtime.hud.set_title(Arc::from("Same"), 1, 100);
    fixture.runtime.hud.set_subtitle(Arc::from("Sub"), 2, 50);
    fixture
        .runtime
        .hud
        .set_actionbar(Arc::from("Action"), 3, 100);
    fixture.runtime.hud.set_tip(Arc::from("Tip"), 4, 100);
    fixture
        .runtime
        .observe_selected_item_identity_value(Some((0, 1, 0)), 100);
    fixture.frame.selected_item_name = Some(Arc::from("Item"));
    let mut cache = SemanticHud::default();
    for now in [100, 200, 349, 350, 400, 2_100, 150] {
        fixture.frame.now_millis = now;
        fixture.check(&mut cache);
    }
    assert_eq!(cache.model.title.as_ref().unwrap().creation_id, 1);
    fixture.runtime.hud.set_durations(ui::TitleDurations {
        fade_in_ticks: 1,
        stay_ticks: 8,
        fade_out_ticks: 1,
    });
    fixture.runtime.hud.set_title(Arc::from("Same"), 5, 150);
    assert!(fixture.check(&mut cache).text);
    let title = cache.model.title.as_ref().unwrap();
    assert_eq!((title.creation_id, title.born, title.stay), (5, 0.15, 0.4));
    fixture.frame.now_millis = 650;
    fixture.check(&mut cache);
    assert!(cache.model.title.is_none());
}

#[test]
fn chat_membership_tracks_future_timestamps_settings_and_both_clock_directions() {
    let mut fixture = Fixture::default();
    fixture.chat("Past", 0);
    fixture.chat("Future", 1_000);
    let mut cache = SemanticHud::default();
    fixture.frame.now_millis = 100;
    fixture.check(&mut cache);
    let text = cache.model.chat[1].text.as_ptr();
    for now in [101, 1_000, 1_001] {
        fixture.frame.now_millis = now;
        fixture.check(&mut cache);
        assert_eq!(cache.model.chat[1].text.as_ptr(), text);
    }
    for now in [100_000, 100] {
        fixture.frame.now_millis = now;
        fixture.check(&mut cache);
    }
    assert_eq!(cache.model.chat[1].born, 0.1);
    fixture.settings.set(SettingId::HideChat.index(), 1);
    assert!(fixture.check(&mut cache).chat);
    assert!(cache.model.chat.is_empty());
    fixture.settings.set(SettingId::HideChat.index(), 0);
    fixture.settings.set(SettingId::ChatColor.index(), 2);
    assert!(fixture.check(&mut cache).chat);
    fixture.settings.set(SettingId::ChatFontSize.index(), 15);
    fixture.settings.set(SettingId::ChatLineSpacing.index(), 2);
    assert!(fixture.check(&mut cache).chat);
    assert_eq!(
        cache.model.chat_text,
        ui_screens::settings_chat::chat_text(&fixture.settings)
    );
    assert_ne!(cache.model.chat_text, json_ui::ChatText::default());
    fixture.runtime.chat_focused = true;
    fixture.check(&mut cache);
    assert!(!cache.model.chat_visible);
    fixture.runtime.chat_focused = false;
    fixture.runtime.inventory_open = true;
    fixture.check(&mut cache);
    assert!(!cache.model.chat_visible);
    fixture.runtime.inventory_open = false;
    fixture.check(&mut cache);
    assert!(cache.model.chat_visible);
    fixture.runtime.chat.clear();
    fixture.chat("Replacement", 0);
    fixture.check(&mut cache);
    assert!(cache.model.chat[0].text.ends_with("Replacement"));
}

#[test]
fn world_text_language_and_visibility_invalidate_without_touching_hotbar() {
    let mut fixture = Fixture::default();
    fixture.runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(true),
        show_days_played: Some(true),
    });
    fixture.frame.player_block = Some([1, 64, -3]);
    fixture.frame.world_time = Some(f64::from(world::TICKS_PER_DAY) * 2.0);
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    fixture.frame.world_time = Some(f64::from(world::TICKS_PER_DAY) * 2.5);
    assert_eq!(fixture.check(&mut cache), HudChanges::default());
    fixture.frame.player_block = Some([2, 64, -3]);
    let changes = fixture.check(&mut cache);
    assert!(changes.world_text && !changes.hotbar);
    let bytes = assets::encode_lang_catalog(
        [1; 32],
        [2; 32],
        &[assets::LangEntry {
            key: "map.position".into(),
            value: "At %s / %s / %s".into(),
        }],
    )
    .unwrap();
    fixture.runtime.set_lang_catalog(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    assert!(fixture.check(&mut cache).world_text);
    assert_eq!(
        cache.model.player_position.as_deref(),
        Some("At 2 / 64 / -3")
    );
    fixture.runtime.last_known_local_player_alive = Some(false);
    fixture.check(&mut cache);
    assert!(cache.model.player_position.is_none());
    fixture
        .player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Spectator);
    fixture.check(&mut cache);
    assert!(cache.model.spectator && !cache.model.hotbar_visible);
    fixture.catalog = Arc::new(Catalog::default());
    assert_eq!(fixture.check(&mut cache), HudChanges::ALL);
    fixture.runtime.begin_session(2);
    fixture.player.begin_session(2);
    assert_eq!(fixture.check(&mut cache), HudChanges::ALL);
}

#[test]
fn sidebar_owner_names_and_boss_changes_rebuild_only_their_components() {
    let mut fixture = Fixture::default();
    fixture
        .runtime
        .scoreboards
        .apply(
            1,
            ui::ScoreboardEvent::DisplayObjective {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("objective"),
                display_name: Arc::from("Score"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            },
        )
        .unwrap();
    fixture
        .runtime
        .scoreboards
        .apply(
            2,
            ui::ScoreboardEvent::Scores {
                entries: vec![ui::ScoreEntry {
                    action: ui::ScoreAction::Change,
                    scoreboard_id: 3,
                    objective_name: Arc::from("objective"),
                    score: 8,
                    owner: ui::ScoreOwner::Player(17),
                }]
                .into(),
            },
        )
        .unwrap();
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    fixture.names.replace([(17, Arc::from("Player"))]);
    assert_eq!(
        fixture.check(&mut cache),
        HudChanges {
            sidebar: true,
            ..Default::default()
        }
    );
    assert_eq!(
        cache.model.sidebar.as_ref().unwrap().rows[0],
        ("Player".into(), "8".into())
    );
    fixture.hide_sidebar = true;
    assert_eq!(
        fixture.check(&mut cache),
        HudChanges {
            sidebar: true,
            ..Default::default()
        }
    );
    assert!(cache.model.sidebar.is_none());
    fixture.hide_sidebar = false;
    fixture.check(&mut cache);
    for (sequence, action, title, health) in [
        (1, ui::BossAction::Show, "Boss", 0.8),
        (2, ui::BossAction::SetProgress, "Boss", 0.2),
        (3, ui::BossAction::SetTitle, "Renamed", 0.2),
        (4, ui::BossAction::Hide, "Renamed", 0.2),
    ] {
        fixture
            .runtime
            .boss_bars
            .apply(
                sequence,
                ui::BossBarEvent {
                    target_entity_id: 9,
                    action,
                    title: Arc::from(title),
                    filtered_title: Arc::from(""),
                    health,
                    style: ui::BossStyle {
                        color: ui::BossColor::Red,
                        overlay: ui::BossOverlay::Notched6,
                        darken_sky: None,
                        create_world_fog: None,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            fixture.check(&mut cache),
            HudChanges {
                boss_bars: true,
                ..Default::default()
            }
        );
    }
}

#[test]
fn offhand_preview_and_mod_visibility_remain_live() {
    let mut fixture = Fixture::default();
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    let event = protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
        container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        slots: vec![stack(2)].into(),
        storage_item: protocol::NetworkItemStack::empty(),
    });
    fixture
        .runtime
        .enqueue_inventory_event(&mut fixture.player, 1, 1, event)
        .unwrap();
    fixture.runtime.drain_pending_inventory(&mut fixture.player);
    fixture.frame.offhand_icon = Some(IconRef {
        page: 5,
        uv: [0, 0, 16, 16],
        glint: false,
    });
    assert!(fixture.check(&mut cache).hotbar);
    fixture.frame.paper_doll_visible = true;
    assert!(fixture.check(&mut cache).status);
    fixture.frame.player_preview = Some(IconRef {
        page: 6,
        uv: [0, 0, 16, 16],
        glint: false,
    });
    assert_eq!(fixture.check(&mut cache), HudChanges::default());
    fixture.settings.set(SettingId::HidePaperdoll.index(), 1);
    assert!(fixture.check(&mut cache).status);
    assert!(!cache.model.paper_doll);
    fixture
        .runtime
        .gameplay_hud
        .apply_effect(protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: protocol::ActorEffectAction::Add,
            effect_id: 10,
            amplifier: 0,
            particles: true,
            ambient: false,
            duration_ticks: -1,
            tick: 0,
        });
    assert!(fixture.check(&mut cache).status);
    fixture.runtime.observe_presentation_tick(10, 0);
    fixture
        .runtime
        .gameplay_hud
        .apply_effect(protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: protocol::ActorEffectAction::Update,
            effect_id: 10,
            amplifier: 0,
            particles: true,
            ambient: false,
            duration_ticks: 2,
            tick: 10,
        });
    fixture.frame.now_millis = 100;
    assert!(fixture.check(&mut cache).status);
    assert!(!cache.model.effects_visible);
    fixture.frame.now_millis = 0;
    assert!(fixture.check(&mut cache).status);
    fixture.hide_effects = true;
    assert!(fixture.check(&mut cache).status);
    assert!(!cache.model.effects_visible);
}

/// Vanilla ignores the local player's own offhand MobEquipment. An echo carrying an empty or
/// older stack must not blank or replace the offhand frame while the inventory holds the item.
#[test]
fn stale_local_offhand_echo_keeps_the_inventory_offhand() {
    let mut fixture = Fixture::default();
    let mut cache = SemanticHud::default();
    let event = protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
        container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        slots: vec![stack(1)].into(),
        storage_item: protocol::NetworkItemStack::empty(),
    });
    fixture
        .runtime
        .enqueue_inventory_event(&mut fixture.player, 1, 1, event)
        .unwrap();
    fixture.runtime.drain_pending_inventory(&mut fixture.player);
    fixture.frame.offhand_icon = Some(IconRef {
        page: 5,
        uv: [0, 0, 16, 16],
        glint: false,
    });
    fixture.check(&mut cache);
    assert_eq!(cache.model.offhand.as_ref().map(|slot| slot.count), Some(1));
    for (sequence, echoed) in [(2, protocol::NetworkItemStack::empty()), (3, stack(3))] {
        fixture.runtime.retain_local_selected_equipment(
            &mut fixture.player,
            sequence,
            protocol::EquipmentEvent {
                actor_runtime_id: 1,
                stack: echoed,
                inventory_slot: 0,
                selected_slot: 0,
                window_id: protocol::OFFHAND_WINDOW_ID as u8,
                handedness: Some(protocol::ActorHandedness::Left),
            },
        );
        fixture.check(&mut cache);
        assert_eq!(cache.model.offhand.as_ref().map(|slot| slot.count), Some(1));
    }
}

#[test]
fn chat_translation_and_duration_refresh_without_rebuilding_hotbar() {
    let mut fixture = Fixture::default();
    fixture.runtime.chat.push_local(ChatMessage {
        fifo_sequence: 0,
        received_millis: 0,
        kind: ChatMessageKind::Translation,
        source: None,
        message: Arc::from("message.key"),
        parameters: Arc::from([]),
    });
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    let bytes = assets::encode_lang_catalog(
        [3; 32],
        [4; 32],
        &[assets::LangEntry {
            key: "message.key".into(),
            value: "Translated".into(),
        }],
    )
    .unwrap();
    fixture.runtime.set_active_language(Some(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    )));
    let changes = fixture.check(&mut cache);
    assert!(changes.chat && !changes.hotbar && !changes.sidebar);
    assert_eq!(cache.model.chat[0].text, "Translated");
    fixture
        .settings
        .set(SettingId::ChatMessageDuration.index(), 0);
    assert!(fixture.check(&mut cache).chat);
    for line in 0..=MAX_CHAT_LINES {
        fixture.chat(&format!("Line {line}"), 0);
    }
    fixture.check(&mut cache);
    assert_eq!(cache.model.chat.len(), MAX_CHAT_LINES);
}

/// Server packs can consume title bindings beyond the native visible label and after its expiry.
#[test]
fn title_controller_inputs_survive_native_title_expiry_without_truncation() {
    let mut fixture = Fixture::default();
    let text = "x".repeat(ui_screens::text::MAX_PRESENTED_TEXT_BYTES + 1);
    fixture
        .runtime
        .hud
        .set_title(Arc::from(text.as_str()), 1, 0);
    fixture
        .runtime
        .hud
        .set_subtitle(Arc::from("retained subtitle"), 2, 0);
    fixture
        .runtime
        .hud
        .set_actionbar(Arc::from(text.as_str()), 3, 0);
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    assert_eq!(cache.model.title.as_ref().unwrap().title, text);
    assert_eq!(cache.model.actionbar.as_ref().unwrap().text, text);
    fixture.frame.now_millis = 60_000;
    fixture.check(&mut cache);
    assert!(cache.model.title.is_none());
    assert_eq!(cache.model.title_text.as_deref(), Some(text.as_str()));
    assert_eq!(
        cache.model.subtitle_text.as_deref(),
        Some("retained subtitle")
    );
}

#[test]
fn chat_color_keeps_encoded_hud_message_inputs_unmodified() {
    let mut fixture = Fixture::default();
    fixture.frame.now_millis = 1000;
    fixture.settings.set(SettingId::ChatColor.index(), 1);
    let message = ui_screens::test_support::hover_chat::message();
    fixture.chat(&message, 1);
    let mut cache = SemanticHud::default();
    fixture.check(&mut cache);
    assert_eq!(cache.model.chat[0].text, message);
    assert_eq!(
        cache.model.chat_color,
        Some(fixture.settings.chat_color_code())
    );
    let catalog = json_ui::Catalog::from_files(
        ui_screens::test_support::hover_chat::files()
            .iter()
            .map(|(path, data)| (path.as_str(), data.as_slice())),
    )
    .unwrap();
    let context = json_ui::Context::empty();
    let root = json_ui::resolve(&catalog, "hud.chat_grid_item", &context)
        .control
        .unwrap();
    let data = json_ui::hud_data_source(&cache.model);
    let bound = json_ui::bind_screen(
        &root,
        &catalog,
        &context,
        &data,
        &mut json_ui::BindState::new(),
    );
    let caption = bound
        .find(&|control: &json_ui::ResolvedControl| control.name == "caption")
        .unwrap();
    assert_eq!(
        caption.properties["text"],
        ui_screens::test_support::hover_chat::CAPTION
    );
}
