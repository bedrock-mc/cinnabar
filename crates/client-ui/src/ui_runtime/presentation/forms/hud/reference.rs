//! The uncached HUD projection used to check semantic retention.

use super::super::super::{HudFrame, bounded_visible_text, resolve_chat_line};
use super::*;
use json_ui::{BossBar, HudModel, HudSlot, HudTitle, Sidebar, Timed};
use ui::{IconRef, TimedText};
use ui_screens::text::bounded_text;

/// What the player sees, as the HUD templates bind it.
pub(super) fn hud_model(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    sidebar: Option<Sidebar>,
    icons: &mut Vec<IconRef>,
    settings: &launcher::menu::settings_options::SettingsOptions,
) -> HudModel {
    let seconds = |millis: u64| millis as f64 / 1_000.0;
    let now = frame.now_millis;
    let mode = player_runtime.facts.player_game_mode();
    let mode_allows_hotbar = mode.is_none_or(|mode| mode.shows_hotbar());
    let selected = player_runtime.selected_hotbar_slot();
    let survival = player_runtime.facts.survival_stats_visible();
    let facts = player_runtime.inventory.ledger().item_facts();
    let mut slot = |stack: Option<&protocol::NetworkItemStack>,
                    icon: Option<IconRef>,
                    durability: Option<f32>,
                    selected: bool| {
        let icon = stack.and(icon).map(|icon| {
            icons.push(icon);
            icons.len() - 1
        });
        HudSlot {
            icon,
            id_aux: stack.map(|stack| facts.id_aux(stack)),
            count: stack.map_or(0, |stack| u32::from(stack.count)),
            selected,
            durability: stack.and(durability).map(f64::from),
        }
    };
    let hotbar = (0..9)
        .map(|index| {
            slot(
                frame.hotbar_stacks[index].as_ref(),
                frame.hotbar_icons[index],
                frame.hotbar_durability[index],
                selected == Some(index as u8),
            )
        })
        .collect();
    let offhand = runtime.gameplay_hud().offhand_stack().map(|stack| {
        slot(
            Some(stack),
            frame.offhand_icon,
            frame.offhand_durability,
            false,
        )
    });
    let experience = runtime.hud().experience();
    let title = visible(runtime.hud().title(), now).map(|title| {
        let fade_in = title.fade_in_millis;
        let fade_out = title.fade_out_millis;
        let total = title.expires_millis.saturating_sub(title.started_millis);
        HudTitle {
            creation_id: title.fifo_sequence,
            title: bounded_text(&title.text, protocol::MAX_UI_TEXT_BYTES).to_owned(),
            subtitle: visible(runtime.hud().subtitle(), now)
                .map(|subtitle| {
                    bounded_text(&subtitle.text, protocol::MAX_UI_TEXT_BYTES).to_owned()
                })
                .unwrap_or_default(),
            fade_in: seconds(fade_in),
            stay: seconds(total.saturating_sub(fade_in + fade_out)),
            fade_out: seconds(fade_out),
            background_alpha: 0.0,
            born: seconds(title.started_millis),
        }
    });
    let timed = |text: &TimedText| Timed {
        text: bounded_text(&text.text, protocol::MAX_UI_TEXT_BYTES).to_owned(),
        born: seconds(text.started_millis),
    };
    let item_name = runtime
        .selected_item_changed_millis()
        .filter(|changed| now.saturating_sub(*changed) < ITEM_NAME_MILLIS)
        .zip(frame.selected_item_name.as_ref())
        .filter(|_| mode_allows_hotbar && selected.is_some())
        .map(|(changed, name)| Timed {
            text: bounded_visible_text(name).to_owned(),
            born: seconds(changed),
        });
    let chat_visible = !runtime.chat_focused() && !runtime.inventory_open();
    let horizon = seconds(now) - settings.chat_lifetime() - 1.0;
    let messages = runtime.chat().messages();
    let chat = messages
        .iter()
        .skip(messages.len().saturating_sub(MAX_CHAT_LINES))
        .filter(|line| seconds(line.received_millis) > horizon)
        .map(|line| {
            let text = resolve_chat_line(line, |key| runtime.translation(key));
            Timed {
                text: bounded_visible_text(text.as_ref()).to_owned(),
                // Rows stamped ahead of the local clock stay fresh.
                born: seconds(line.received_millis.min(now)),
            }
        })
        .collect();
    let now_tick = runtime.estimated_server_tick(now);
    let (player_position, days_played) = world_text_lines(runtime, frame);
    HudModel {
        survival_ui: survival,
        armor_visible: runtime
            .hud()
            .armor()
            .is_some_and(|armor| armor.current() > 0),
        hotbar_visible: mode_allows_hotbar && selected.is_some(),
        touch_inventory: false,
        xp_bar: survival && experience.is_some() && frame.mount_jump.is_none(),
        exp_progress: experience.map_or(0.0, |xp| f64::from(xp.progress)),
        level: experience.map_or(0, |xp| xp.level),
        hotbar,
        offhand,
        riding_hearts: survival && frame.mount_health.is_some(),
        bubbles_visible: survival
            && runtime
                .hud()
                .air()
                .is_some_and(|air| air.current() < air.maximum()),
        paper_doll: frame.paper_doll_visible
            && mode_allows_hotbar
            && settings.value(SettingId::HidePaperdoll) == 0,
        effects_visible: runtime
            .gameplay_hud()
            .effects()
            .iter()
            .any(|effect| effect.visible_at_tick(now_tick)),
        spectator: !mode_allows_hotbar,
        title,
        title_text: Some(
            bounded_text(runtime.hud().title_text(), protocol::MAX_UI_TEXT_BYTES).to_owned(),
        ),
        subtitle_text: Some(
            bounded_text(runtime.hud().subtitle_text(), protocol::MAX_UI_TEXT_BYTES).to_owned(),
        ),
        actionbar: visible(runtime.hud().actionbar(), now).map(timed),
        tip: visible(runtime.hud().tip(), now).map(timed),
        item_name,
        chat,
        chat_visible,
        chat_color: ui_screens::settings_chat::default_color(settings),
        chat_lifetime: settings.chat_lifetime(),
        chat_background_opacity: CHAT_BACKGROUND_OPACITY,
        chat_text: ui_screens::settings_chat::chat_text(settings),
        sidebar,
        boss_bars: runtime
            .boss_bars()
            .stacked_iter()
            .take(MAX_BOSS_BARS)
            .map(|bar| BossBar {
                name: bounded_visible_text(&bar.title).to_owned(),
                progress: f64::from(bar.health),
                color: boss_tint(bar.style.color),
                notches: match bar.style.overlay {
                    ui::BossOverlay::Progress => 0,
                    ui::BossOverlay::Notched6 => 6,
                    ui::BossOverlay::Notched10 => 10,
                    ui::BossOverlay::Notched12 => 12,
                    ui::BossOverlay::Notched20 => 20,
                },
            })
            .collect(),
        player_position,
        days_played,
        text_background_alpha: f64::from(settings.value(SettingId::HudTextBackgroundOpacity))
            / 100.0,
    }
}

/// The position line (the `showcoordinates` rule or a held filled map) and the
/// days-played line (`showdaysplayed`), both hidden while the player is dead.
pub(super) fn world_text_lines(
    runtime: &UiRuntime,
    frame: &HudFrame,
) -> (Option<String>, Option<String>) {
    let alive = runtime.last_known_local_player_alive.unwrap_or(true);
    let rules = runtime.gameplay_hud();
    let translate = |key: &str, fallback: &str, arguments: &[String]| {
        let template = runtime
            .translation(key)
            .map_or_else(|| fallback.to_owned(), |text| text.to_string());
        protocol::format_translation(&template, arguments)
    };
    let position = frame
        .player_block
        .filter(|_| alive && (rules.show_coordinates() || frame.holding_filled_map))
        .map(|block| {
            translate(
                "map.position",
                "Position: %s, %s, %s",
                &block.map(|axis| axis.to_string()),
            )
        });
    let days = frame
        .world_time
        .filter(|_| alive && rules.show_days_played())
        .map(|time| {
            let days = (time / f64::from(world::TICKS_PER_DAY)).floor();
            if days < 0.0 {
                translate("hudScreen.daysPlayed.overflow", "Too many to count!", &[])
            } else {
                translate(
                    "hudScreen.daysPlayed",
                    "Days played: %s",
                    &[format!("{days:.0}")],
                )
            }
        });
    (position, days)
}

/// Applies the controller expiry boundary.
fn visible(text: Option<&TimedText>, now: u64) -> Option<&TimedText> {
    text.filter(|text| text.visible_at(now))
}

/// Formats the authoritative sidebar projection for the uncached path.
pub(super) fn sidebar_model(
    scoreboard: &super::super::super::retained_hud::PresentedScoreboard,
) -> Sidebar {
    use super::super::super::retained_hud::PresentedScoreValue;
    Sidebar {
        title: bounded_visible_text(&scoreboard.title).to_owned(),
        rows: scoreboard
            .rows
            .iter()
            .map(|row| {
                let score = match &row.value {
                    PresentedScoreValue::Text(text) => bounded_visible_text(text).to_owned(),
                    PresentedScoreValue::Hearts {
                        full_hearts,
                        half_heart,
                    } => (u32::from(*full_hearts) * 2 + u32::from(*half_heart)).to_string(),
                };
                (bounded_visible_text(&row.label).to_owned(), score)
            })
            .collect(),
        background_opacity: SIDEBAR_OPACITY,
        title_background_opacity: SIDEBAR_TITLE_OPACITY,
    }
}

/// Formats the native renderer palette as a template tint.
fn boss_tint(color: ui::BossColor) -> String {
    let [r, g, b, _] = hud_layout::BOSS_TINTS
        .iter()
        .find(|(tint, _)| *tint == color)
        .map_or([255; 4], |(_, rgba)| *rgba);
    format!("#{r:02x}{g:02x}{b:02x}")
}
