//! Projects the frame into the engine HUD: the [`HudModel`] the templates bind, the native
//! renderers' captured paint, and the mod and preview renderers drawn ahead of the HUD's own.

use std::{collections::BTreeSet, sync::Arc};

use json_ui::{
    BossBar, CROSSHAIR_SCREEN, Catalog, HUD_SCREEN, HudModel, HudSlot, HudTitle, Sidebar, Timed,
    hud_context,
};
use mod_ui::crosshair::CrosshairRenderer;
use ui::{IconRef, TimedText, UiNode};
use ui_engine::{
    engine::screen::{CustomRenderer, PaintFrame},
    text_metrics::TextMetrics,
};
use ui_screens::hud::HudInput;

use super::super::{
    HudFrame, UiPresentationError, UiPresentationRuntime, bounded_visible_text, hud_layout,
    resolve_chat_line,
};
use super::preview_renderer::PreviewRenderer;
use crate::ui_runtime::UiRuntime;

#[cfg(test)]
mod cooldown_tests;
#[cfg(test)]
mod crosshair_tests;
#[cfg(test)]
mod hunger_control_tests;

/// Java's per-line chat background opacity.
const CHAT_BACKGROUND_OPACITY: f64 = 0.5;
/// Newest chat lines the controller keeps alive.
const MAX_CHAT_LINES: usize = 50;
/// Java sidebar background opacities: 0.3 for rows, 0.4 for the title.
const SIDEBAR_OPACITY: f64 = 0.3;
const SIDEBAR_TITLE_OPACITY: f64 = 0.4;
/// The selected-item label shows for two seconds after the selection changes.
const ITEM_NAME_MILLIS: u64 = 2_000;
/// Display cap for stacked boss bars; the retained store holds more.
const MAX_BOSS_BARS: usize = 8;

/// Resolves the HUD screens of `catalog` under both chat placements on the calling thread, and
/// returns the textures their controls name.
pub(super) fn prepare(catalog: &Arc<Catalog>) -> BTreeSet<String> {
    let base = hud_context(&ui_screens::menu_screens::retail_context());
    ui_screens::hud::prepared::prepare(
        catalog,
        [false, true].map(|top| ui_screens::chat_position::placed(base.clone(), top)),
    )
}

impl UiPresentationRuntime {
    /// Draw the gameplay HUD screen, or its crosshair overlay screen, through
    /// the engine; `Ok(false)` when the engine is not loaded.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_engine_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
        crosshair: bool,
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        if self.screens.chat.settings.options().value("hide_hud") != 0 {
            return Ok(true);
        }
        let context = ui_screens::chat_position::context(
            hud_context(renderer.context()),
            self.screens.chat.settings.options(),
        );
        let reference = if crosshair {
            CROSSHAIR_SCREEN
        } else {
            HUD_SCREEN
        };
        if !renderer
            .scene_settings(reference, &context)
            .renders(crosshair || !runtime.chat_focused())
        {
            return Ok(true);
        }
        let mut frame = self.hud_frame.clone();
        frame.now_millis = now_millis;
        let mut icons = Vec::new();
        let model = if crosshair {
            None
        } else {
            let sidebar = self
                .scoreboard
                .refresh(runtime.scoreboards(), &self.scoreboard_owner_names)
                .map(sidebar_model);
            let sidebar = if self.screens.mod_ui.mod_scoreboard_hidden() {
                None
            } else {
                sidebar
            };
            let options = self.screens.chat.settings.options();
            let mut model = hud_model(
                player_runtime,
                runtime,
                &frame,
                sidebar,
                &mut icons,
                options,
            );
            if self.screens.mod_ui.mod_effect_icons_hidden() {
                model.effects_visible = false;
            }
            ui_screens::settings_chat::apply_hud(options, &mut model);
            Some((model, options.value("interface_opacity")))
        };
        let catalog = Arc::clone(renderer.catalog());
        let data = self
            .screens
            .hud
            .begin(model, &self.scene_clock, runtime.session_id(), &catalog);
        let mut paint = hud_layout::capture_hud_paint(
            player_runtime,
            runtime,
            &frame,
            self.hud_textures.as_ref(),
            self.screens.chat.settings.options(),
        );
        if let Some(style) = self.screens.mod_ui.mod_hunger_overlay() {
            paint.hunger.set_overlay(
                style,
                runtime.gameplay_hud().saturation(),
                frame.selected_food.as_ref(),
            );
        }
        // Packs may replace the native cursor renderer with ordinary image controls.
        // Gate the whole overlay so these controls obey the same camera and HUD settings.
        if crosshair && paint.crosshair.is_none() {
            return Ok(true);
        }
        if !crosshair {
            // Legacy HUDs embed this renderer; the separate overlay already draws it.
            // Inverting the same pixels twice restores the world behind the cursor.
            paint.crosshair = None;
        }
        if self.screens.mod_ui.mod_effect_icons_hidden() {
            paint.effects.clear();
        }
        let crosshair_spec = if self.mod_hud_visible(player_runtime, runtime) {
            self.screens.mod_ui.mod_crosshair().cloned()
        } else {
            None
        };
        let translate = |key: &str| runtime.translation(key);
        let preview_view = std::cell::Cell::new(None);
        let mut preview = PreviewRenderer {
            preview: frame.player_preview,
            view: Some(&preview_view),
            ..PreviewRenderer::default()
        };
        let mut custom_crosshair = CrosshairRenderer {
            spec: crosshair_spec.as_ref(),
            visible: paint.crosshair.is_some(),
        };
        let mut renderers: [&mut dyn CustomRenderer; 2] = [&mut custom_crosshair, &mut preview];
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: now_millis as f64 / 1_000.0,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut renderers,
        };
        self.screens.hud.paint(
            renderer,
            HudInput {
                crosshair,
                catalog: &catalog,
                context: &context,
                data,
                paint: &paint,
                icons: &icons,
            },
            &mut paint_frame,
        )?;
        if let Some(view) = preview_view.get() {
            self.player_preview_view = view;
        }
        Ok(true)
    }
}

/// What the player sees, as the HUD templates bind it.
fn hud_model(
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
            title: bounded_visible_text(&title.text).to_owned(),
            subtitle: visible(runtime.hud().subtitle(), now)
                .map(|subtitle| bounded_visible_text(&subtitle.text).to_owned())
                .unwrap_or_default(),
            fade_in: seconds(fade_in),
            stay: seconds(total.saturating_sub(fade_in + fade_out)),
            fade_out: seconds(fade_out),
            background_alpha: 0.0,
            born: seconds(title.started_millis),
        }
    });
    let timed = |text: &TimedText| Timed {
        text: bounded_visible_text(&text.text).to_owned(),
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
                text: ui_screens::settings_chat::message_text(settings, text.as_ref()),
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
            && settings.value("hide_paperdoll") == 0,
        effects_visible: runtime
            .gameplay_hud()
            .effects()
            .iter()
            .any(|effect| effect.visible_at_tick(now_tick)),
        spectator: !mode_allows_hotbar,
        title,
        actionbar: visible(runtime.hud().actionbar(), now).map(timed),
        tip: visible(runtime.hud().tip(), now).map(timed),
        item_name,
        chat,
        chat_visible,
        chat_lifetime: settings.chat_lifetime(),
        chat_background_opacity: CHAT_BACKGROUND_OPACITY,
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
        text_background_alpha: f64::from(settings.value("hud_text_background_opacity")) / 100.0,
    }
}

/// The position line (the `showcoordinates` rule or a held filled map) and the
/// days-played line (`showdaysplayed`), both hidden while the player is dead.
fn world_text_lines(runtime: &UiRuntime, frame: &HudFrame) -> (Option<String>, Option<String>) {
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

fn visible(text: Option<&TimedText>, now: u64) -> Option<&TimedText> {
    text.filter(|text| text.visible_at(now))
}

fn sidebar_model(scoreboard: &super::super::retained_hud::PresentedScoreboard) -> Sidebar {
    use super::super::retained_hud::PresentedScoreValue;
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

fn boss_tint(color: ui::BossColor) -> String {
    let [r, g, b, _] = hud_layout::BOSS_TINTS
        .iter()
        .find(|(tint, _)| *tint == color)
        .map_or([255; 4], |(_, rgba)| *rgba);
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(any(test, feature = "test-support"))]
impl UiPresentationRuntime {
    /// The engine HUD's last laid-out draw nodes, in GUI px.
    pub fn hud_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.hud.hud.nodes()
    }

    /// Bind+layout passes the engine HUD ran.
    pub fn hud_passes(&self) -> usize {
        self.screens.hud.hud.passes
    }

    /// The engine HUD's painted sprite paths that resolve to no texture source.
    pub fn hud_unresolved_sprites(&self) -> Vec<String> {
        let Some(engine) = self.engine.as_deref() else {
            return Vec::new();
        };
        let atlas = engine.textures.lock();
        let view = ui_engine::textures::Textures {
            assets: engine.assets(),
            set: &engine.textures,
            atlas: &atlas,
            images: None,
        };
        let mut missing: Vec<String> = self
            .hud_draw_nodes()
            .iter()
            .filter(|node| node.alpha > 0.0)
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Sprite { texture, .. } => Some(texture.clone()),
                _ => None,
            })
            .filter(|texture| view.sprite(texture).is_none())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// A draw node's fade multiplier at `now`, under this frame's clocks,
    /// sampled by a fresh animator so it runs from the node's creation clock.
    pub fn hud_fade(&self, node: &json_ui::DrawNode, now: f64) -> f32 {
        let clocks = Some(self.screens.hud.clocks());
        let opacity = node
            .animate(&mut json_ui::Animator::new(), now, clocks, None)
            .opacity;
        if node.alpha > 0.0 {
            opacity / node.alpha
        } else {
            opacity
        }
    }
}

#[cfg(test)]
#[path = "hud/cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "hud/texture_metadata_tests.rs"]
mod texture_metadata_tests;

#[cfg(test)]
#[path = "hud/life_state_tests.rs"]
mod life_state_tests;
