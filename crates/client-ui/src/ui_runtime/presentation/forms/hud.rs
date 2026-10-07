//! The gameplay HUD through the JSON-UI engine: the frame's state becomes a
//! [`HudModel`] bound to `hud.hud_screen` and the crosshair overlay over the
//! session's pack stack (the built-in Java HUD pack at the bottom). The bound
//! and laid-out screen is reused until the model, catalog, viewport, or scale
//! changes; each frame only repaints it, evaluating fades and the native
//! renderers against the live state.

use std::sync::Arc;

use json_ui::{
    BindState, BossBar, CROSSHAIR_SCREEN, CachedLibrary, Catalog, CatalogLibrary, Context,
    DataSource, FormRender, HUD_SCREEN, HudModel, HudSlot, HudTitle, ResolveCache, ResolvedControl,
    Sidebar, Timed, ViewState, bind_incremental, hud_clocks, hud_context, hud_data_source, rebind,
    render_bound_cached, resolve,
};
use ui::{TimedText, UiNode};

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, HudFrame, IconRef, TextMetrics, UiPresentationError,
    UiPresentationRuntime, bounded_visible_text, hud_layout, resolve_chat_line,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use crate::ui_runtime::UiRuntime;

/// The built-in Java-styled HUD pack: `(pack path, namespace, bytes)`, layered
/// under every server pack.
pub(super) const JAVA_HUD_PACK: [(&str, &str, &[u8]); 4] = [
    (
        "ui/_global_variables.json",
        "",
        include_bytes!("../../../../../../assets/java-hud/ui/_global_variables.json"),
    ),
    (
        "ui/chat_screen.json",
        "chat",
        include_bytes!("../../../../../../assets/java-hud/ui/chat_screen.json"),
    ),
    (
        "ui/hud_screen.json",
        "hud",
        include_bytes!("../../../../../../assets/java-hud/ui/hud_screen.json"),
    ),
    (
        "ui/scoreboards.json",
        "scoreboard",
        include_bytes!("../../../../../../assets/java-hud/ui/scoreboards.json"),
    ),
];

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
/// Ticks in one Minecraft day.
const TICKS_PER_DAY: f64 = 24_000.0;

/// One screen's resolved tree per catalog and its last layout per model.
#[derive(Default)]
pub(super) struct CachedScreen {
    resolved: Option<ResolvedScreen>,
    /// Factory and grid resolutions for the resolved catalog, kept across binds.
    library: ResolveCache,
    /// The screen's live bindings across data refreshes.
    binding: BindState,
    laid: Option<Laid>,
    measures: json_ui::MeasureCache,
    /// Bind+layout passes run, for cache tests and profiling.
    pub(super) passes: usize,
}

struct ResolvedScreen {
    reference: String,
    catalog: Arc<Catalog>,
    context: Context,
    tree: Option<Arc<ResolvedControl>>,
}

struct Laid {
    reference: String,
    catalog: Arc<Catalog>,
    data: Arc<DataSource>,
    view: ViewState,
    root: [f64; 2],
    px: f32,
    language: [usize; 3],
    render: FormRender,
}

impl CachedScreen {
    /// Texture metadata participates in layout, including tiled sprite sizes.
    fn invalidate_textures(&mut self) {
        self.laid = None;
        self.measures = json_ui::MeasureCache::default();
    }

    /// Whether the bound HUD has content for an extension to accompany.
    pub(super) fn has_visible_content(&self) -> bool {
        self.laid.as_ref().is_some_and(|laid| {
            laid.render
                .nodes
                .iter()
                .any(|node| node.alpha > 0.0 && node.shown(&laid.view))
        })
    }

    /// The laid-out screen for `data`, rebinding only when an input changed.
    fn render(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: Arc<DataSource>,
        at: ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
    ) -> Option<&FormRender> {
        self.render_shared_with(
            reference,
            catalog,
            context,
            data,
            at,
            env,
            &ViewState::default(),
        )
    }

    /// [`Self::render`] under the caller's pointer, focus and scroll state.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_with(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: DataSource,
        (root, px, language): ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
        view: &ViewState,
    ) -> Option<&FormRender> {
        self.render_shared_with(
            reference,
            catalog,
            context,
            Arc::new(data),
            (root, px, language),
            env,
            view,
        )
    }

    /// Reuse a controller revision without cloning or comparing all its bags.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_shared_with(
        &mut self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        data: Arc<DataSource>,
        (root, px, language): ([f64; 2], f32, [usize; 3]),
        env: &json_ui::LayoutEnv,
        view: &ViewState,
    ) -> Option<&FormRender> {
        let fresh = self.laid.as_ref().is_some_and(|laid| {
            laid.reference == reference
                && Arc::ptr_eq(&laid.catalog, catalog)
                && laid.root == root
                && laid.px == px
                && laid.language == language
                && self
                    .resolved
                    .as_ref()
                    .is_some_and(|resolved| resolved.context == *context)
                && (Arc::ptr_eq(&laid.data, &data) || laid.data == data)
                && laid.view == *view
        });
        if !fresh {
            let current = self.resolved.as_ref().is_some_and(|resolved| {
                resolved.reference == reference
                    && Arc::ptr_eq(&resolved.catalog, catalog)
                    && resolved.context == *context
            });
            if !current {
                let tree = resolve(catalog, reference, context).control.map(Arc::new);
                self.resolved = Some(ResolvedScreen {
                    reference: reference.to_owned(),
                    catalog: Arc::clone(catalog),
                    context: context.clone(),
                    tree,
                });
                self.library = ResolveCache::default();
                self.binding = BindState::new();
            }
            let tree = self.resolved.as_ref()?.tree.as_ref()?;
            let library = CachedLibrary {
                library: CatalogLibrary { catalog, context },
                cache: &self.library,
            };
            let previous = self.laid.take().filter(|_| current);
            let same_frame = previous.as_ref().is_some_and(|laid| {
                laid.root == root && laid.px == px && laid.language == language
            });
            let bound = match previous {
                Some(laid) => {
                    let mut bound = laid.render.bound;
                    if !same_frame {
                        self.measures = json_ui::MeasureCache::default();
                    }
                    rebind(
                        tree,
                        &data,
                        &library,
                        &mut self.binding,
                        &mut bound,
                        &mut self.measures,
                    );
                    bound
                }
                None => {
                    self.measures = json_ui::MeasureCache::default();
                    bind_incremental(tree, &data, &library, &mut self.binding)
                }
            };
            self.passes += 1;
            self.laid = Some(Laid {
                reference: reference.to_owned(),
                catalog: Arc::clone(catalog),
                render: render_bound_cached(bound, root, env, view, &mut self.measures),
                data,
                view: view.clone(),
                root,
                px,
                language,
            });
        }
        self.laid.as_ref().map(|laid| &laid.render)
    }
}

/// The HUD and crosshair screens, carried across frames.
#[derive(Default)]
pub(super) struct HudScreens {
    pub(super) hud: CachedScreen,
    crosshair: CachedScreen,
    /// The toast screen, drawn above everything in game.
    pub(super) toast: CachedScreen,
    /// The world-loading screen shown while joining.
    pub(super) loading: CachedScreen,
    /// This frame's fade clocks (title, action bar, item name).
    clocks: std::collections::BTreeMap<String, f64>,
    model: Option<HudModel>,
    opacity: Option<i32>,
    data: Arc<DataSource>,
}

impl HudScreens {
    /// Preserve bindings while relaying out screens for a changed texture pack.
    pub(super) fn invalidate_textures(&mut self) {
        self.hud.invalidate_textures();
        self.crosshair.invalidate_textures();
        self.toast.invalidate_textures();
        self.loading.invalidate_textures();
    }
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
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(false);
        };
        if self
            .form_presentation
            .chat
            .settings
            .options
            .value("hide_hud")
            != 0
        {
            return Ok(true);
        }
        let mut frame = self.hud_frame.clone();
        frame.now_millis = now_millis;
        let mut icons = Vec::new();
        let data = if crosshair {
            Arc::default()
        } else {
            let sidebar = self
                .scoreboard
                .refresh(runtime.scoreboards(), &self.scoreboard_owner_names)
                .map(sidebar_model);
            let options = &self.form_presentation.chat.settings.options;
            let mut model = hud_model(
                player_runtime,
                runtime,
                &frame,
                sidebar,
                &mut icons,
                options,
            );
            super::settings_chat::apply_hud(options, &mut model);
            let opacity = options.value("interface_opacity");
            let hud = &mut self.form_presentation.hud;
            hud.clocks = hud_clocks(&model);
            if hud.model.as_ref() != Some(&model) || hud.opacity != Some(opacity) {
                let mut data = hud_data_source(&model);
                data.set_global(
                    "#hud_alpha",
                    json_ui::Scalar::Num(f64::from(opacity) / 100.0),
                );
                data.set_global("#hud_propagate_alpha", json_ui::Scalar::Bool(true));
                hud.data = Arc::new(data);
                hud.opacity = Some(opacity);
                hud.model = Some(model);
            }
            Arc::clone(&hud.data)
        };
        self.form_presentation
            .hud
            .clocks
            .extend(self.scene_clock.clone());
        let paint = hud_layout::capture_hud_paint(
            player_runtime,
            runtime,
            &frame,
            self.hud_textures.as_ref(),
            &self.form_presentation.chat.settings.options,
        );
        let context = hud_context(renderer.context());
        let catalog = Arc::clone(renderer.catalog());
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let translate = |key: &str| runtime.translation(key);
        let screens = &mut self.form_presentation.hud;
        let preview_view = std::cell::Cell::new(None);
        let art = ScreenArt {
            icons: &icons,
            now: now_millis as f64 / 1_000.0,
            hud: Some(&paint),
            preview: frame.player_preview,
            preview_view: Some(&preview_view),
            clocks: Some(&screens.clocks),
            ..ScreenArt::default()
        };
        let (reference, screen) = if crosshair {
            (CROSSHAIR_SCREEN, &mut screens.crosshair)
        } else {
            (HUD_SCREEN, &mut screens.hud)
        };
        if !renderer
            .scene_settings(reference, &context)
            .renders(crosshair || !runtime.chat_focused())
        {
            return Ok(true);
        }
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = EngineOutput {
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &[],
        };
        renderer.draw(art, inputs, out, |env, root| {
            screen.render(
                reference,
                &catalog,
                &context,
                data,
                (root, px, runtime.text_generation()),
                env,
            )
        })?;
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
    settings: &crate::menu::settings_options::SettingsOptions,
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
                text: super::settings_chat::message_text(settings, text.as_ref()),
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
    let alive = runtime
        .hud()
        .health()
        .is_none_or(|health| health.current() > 0);
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
            let days = (time / TICKS_PER_DAY).floor();
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

impl CachedScreen {
    /// The last laid-out draw nodes, in virtual px.
    pub(super) fn nodes(&self) -> &[json_ui::DrawNode] {
        self.laid
            .as_ref()
            .map_or(&[], |laid| laid.render.nodes.as_slice())
    }
}

#[cfg(any(test, feature = "test-support"))]
impl UiPresentationRuntime {
    /// The engine HUD's last laid-out draw nodes, in GUI px.
    pub fn hud_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.form_presentation.hud.hud.nodes()
    }

    /// Bind+layout passes the engine HUD ran.
    pub fn hud_passes(&self) -> usize {
        self.form_presentation.hud.hud.passes
    }

    /// The engine HUD's painted sprite paths that resolve to no texture source.
    pub fn hud_unresolved_sprites(&self) -> Vec<String> {
        let Some(engine) = self.form_presentation.engine.as_deref() else {
            return Vec::new();
        };
        let atlas = engine.textures.lock();
        let view = super::textures::Textures {
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
        let clocks = Some(&self.form_presentation.hud.clocks);
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
