//! The gameplay HUD's controller data: a [`HudModel`] of what the player sees,
//! mapped onto the `#bindings`, collections, and factory-created controls the
//! vanilla `hud_screen.json`, `scoreboards.json`, and `hud_crosshair_overlay.json`
//! read, as the client's HUD and scoreboard screen controllers feed them.
//! Binding and property-bag names match those files and the vanilla HUD controller.

use serde_json::Value;

use crate::Context;
use crate::bind::{CollectionItem, DataSource, FactoryItem};
use crate::predicate::Scalar;

/// The HUD screen and the crosshair overlay the client stacks above it.
pub const HUD_SCREEN: &str = "hud.hud_screen";
pub const CROSSHAIR_SCREEN: &str = "hud_crosshair.hud_crosshair_screen";

/// Everything the HUD templates bind, as plain values. Times are seconds on the
/// caller's animation clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudModel {
    pub survival_ui: bool,
    pub armor_visible: bool,
    pub hotbar_visible: bool,
    /// The classic experience bar shows (else the hotbar stands alone).
    pub xp_bar: bool,
    pub exp_progress: f64,
    pub level: u32,
    pub hotbar: Vec<HudSlot>,
    /// Not a vanilla binding: the built-in Java pack's offhand slot reads it as
    /// the `offhand_items` collection and `#offhand_visible`.
    pub offhand: Option<HudSlot>,
    pub riding_hearts: bool,
    pub bubbles_visible: bool,
    pub paper_doll: bool,
    pub effects_visible: bool,
    pub spectator: bool,
    pub title: Option<HudTitle>,
    pub actionbar: Option<Timed>,
    pub item_name: Option<Timed>,
    pub chat: Vec<Timed>,
    pub chat_visible: bool,
    /// Seconds a chat line stays before its fade.
    pub chat_lifetime: f64,
    pub chat_background_opacity: f64,
    pub sidebar: Option<Sidebar>,
    pub boss_bars: Vec<BossBar>,
    /// The `Position: x, y, z` line, when the world shows coordinates.
    pub player_position: Option<String>,
    /// The `Days played: n` line, when the world shows days played.
    pub days_played: Option<String>,
    /// Opacity of the backgrounds behind the position and days lines.
    pub text_background_alpha: f64,
}

/// One hotbar cell: an index into the caller's icon table, the count, and the
/// durability fraction of a damageable item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudSlot {
    pub icon: Option<usize>,
    pub count: u32,
    pub selected: bool,
    pub durability: Option<f64>,
}

/// Text and when it was set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timed {
    pub text: String,
    pub born: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudTitle {
    /// Identity of the controller request that created this title.
    pub creation_id: u64,
    pub title: String,
    pub subtitle: String,
    pub fade_in: f64,
    pub stay: f64,
    pub fade_out: f64,
    /// The text-background opacity setting behind titles.
    pub background_alpha: f64,
    pub born: f64,
}

/// The displayed sidebar objective, rows already sorted and formatted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sidebar {
    pub title: String,
    pub rows: Vec<(String, String)>,
    pub background_opacity: f64,
    pub title_background_opacity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BossBar {
    pub name: String,
    pub progress: f64,
    /// `#rrggbb` tint of the filled bar.
    pub color: String,
    /// Segments the bar's overlay divides it into (0 for a plain bar); not a
    /// vanilla binding: the built-in Java pack reads it as `#bar_notches`.
    pub notches: u32,
}

/// The HUD's screen context: `base` with the edition and editor flags the
/// templates' `ignored`/`requires` read.
pub fn hud_context(base: &Context) -> Context {
    base.clone()
        .with_flag("education_edition", false)
        .with_flag("is_editor_mode_enabled", false)
        .with_flag("compress_hud_width", false)
}

const TITLE_CLOCK: &str = "hud_title_text";
const ACTIONBAR_CLOCK: &str = "hud_actionbar_text";
const ITEM_NAME_CLOCK: &str = "item_name_text";

/// When the title, action bar, and item name were last set, the clocks their
/// fades read at paint time (so a re-send does not re-bind the screen).
pub fn hud_clocks(model: &HudModel) -> std::collections::BTreeMap<String, f64> {
    [
        (TITLE_CLOCK, model.title.as_ref().map(|title| title.born)),
        (
            ACTIONBAR_CLOCK,
            model.actionbar.as_ref().map(|bar| bar.born),
        ),
        (
            ITEM_NAME_CLOCK,
            model.item_name.as_ref().map(|item| item.born),
        ),
    ]
    .into_iter()
    .filter_map(|(clock, born)| Some((clock.to_owned(), born?)))
    .collect()
}

/// Map `model` onto the HUD templates' bindings.
pub fn hud_data_source(model: &HudModel) -> DataSource {
    let mut data = DataSource::new();
    // Unbound flags read as false, as the controller answers them.
    data.set_strict(true);
    let flag = |data: &mut DataSource, name: &str, value: bool| {
        data.set_global(name, Scalar::Bool(value));
    };
    for (name, value) in [
        ("#hud_visible", true),
        ("#hud_visible_centered", true),
        ("#hud_visible_centered_gui_elements", true),
        ("#show_survival_ui", model.survival_ui),
        (
            "#is_armor_visible",
            model.armor_visible && model.survival_ui,
        ),
        ("#hotbar_visible", model.hotbar_visible),
        ("#hotbar_with_xp_bar", model.xp_bar),
        ("#hotbar_no_xp_bar", !model.xp_bar),
        ("#level_number_visible", model.xp_bar && model.level > 0),
        ("#is_spectator_mode", model.spectator),
        ("#survival_horse_hearts", model.riding_hearts),
        (
            "#is_not_riding_bubbles",
            model.bubbles_visible && !model.riding_hearts,
        ),
        (
            "#is_riding_bubbles",
            model.bubbles_visible && model.riding_hearts,
        ),
        ("#paper_doll_visible", model.paper_doll),
        ("#status_effects_visible", model.effects_visible),
        ("#scoreboard_sidebar_visible", model.sidebar.is_some()),
        ("#chat_visible", model.chat_visible),
    ] {
        flag(&mut data, name, value);
    }
    data.set_global("#hud_alpha", Scalar::Num(1.0));
    // Both bars bind a clip ratio: the fraction clipped away.
    data.set_global(
        "#exp_progress",
        Scalar::Num(1.0 - model.exp_progress.clamp(0.0, 1.0)),
    );
    data.set_global("#level_number", Scalar::Text(model.level.to_string()));
    data.set_global("#gamertag", Scalar::Text(String::new()));
    data.set_grid_dimensions("#hotbar_grid_dimensions", [model.hotbar.len() as u32, 1]);
    data.set_collection("hotbar_items", model.hotbar.iter().map(slot_item).collect());
    data.set_global(
        "#offhand_visible",
        Scalar::Bool(model.offhand.is_some() && model.hotbar_visible),
    );
    data.set_collection(
        "offhand_items",
        model.offhand.iter().map(slot_item).collect(),
    );
    titles(&mut data, model);
    if let Some(item) = &model.item_name {
        data.set_global("#item_text", Scalar::Text(item.text.clone()));
        data.set_factory(
            "item_text_factory",
            vec![
                FactoryItem::new("item_text", 0.0)
                    .clocked(ITEM_NAME_CLOCK)
                    .named("item_name_text")
                    .var("localize", Value::Bool(false))
                    .var("show_survival_padding", Value::Bool(model.survival_ui))
                    .var("show_text_background", Value::Bool(false))
                    .var("item_text_background_alpha", Value::from(0.0)),
            ],
        );
    }
    if model.chat_visible {
        data.set_collection(
            "chat_text_grid",
            model
                .chat
                .iter()
                .map(|line| {
                    CollectionItem::default().with("#chat_text", Scalar::Text(line.text.clone()))
                })
                .collect(),
        );
        let chat = model
            .chat
            .iter()
            .enumerate()
            .map(|(index, line)| {
                FactoryItem::new("chat_item", line.born)
                    .named("chat_grid_item")
                    .at("chat_text_grid", index)
                    .value("#text", Scalar::Text(line.text.clone()))
                    .var("chat_item_lifetime", Value::from(model.chat_lifetime))
                    .var(
                        "chat_background_opacity",
                        Value::from(model.chat_background_opacity),
                    )
                    .var("chat_font_type", Value::from("default"))
                    .var("chat_font_scale_factor", Value::from(1.0))
                    .var("chat_line_spacing", Value::from(0.0))
            })
            .collect();
        data.set_factory("chat_item_factory", chat);
    }
    sidebar(&mut data, model.sidebar.as_ref());
    boss_bars(&mut data, &model.boss_bars);
    for (line, visible, text) in [
        (
            &model.player_position,
            "#player_position_visible",
            "#player_position_text",
        ),
        (
            &model.days_played,
            "#number_of_days_played_visible",
            "#number_of_days_played_text",
        ),
    ] {
        data.set_global(visible, Scalar::Bool(line.is_some()));
        if let Some(line) = line {
            data.set_global(text, Scalar::Text(line.clone()));
        }
    }
    data.set_global(
        "#hud_text_background_alpha",
        Scalar::Num(model.text_background_alpha),
    );
    data
}

fn slot_item(slot: &HudSlot) -> CollectionItem {
    CollectionItem::default()
        // Retained bindings keep an unanswered value. Null explicitly clears
        // our optional icon reference; it is not a native numeric item sentinel.
        .with(
            "#item_renderer_data",
            slot.icon
                .map_or(Scalar::Json(Value::Null), |icon| Scalar::Num(icon as f64)),
        )
        .with("#slot_selected", Scalar::Bool(slot.selected))
        .with(
            "#inventory_stack_count",
            Scalar::Text(if slot.count > 1 {
                slot.count.to_string()
            } else {
                String::new()
            }),
        )
        .with("#stack_count_visible", Scalar::Bool(slot.count > 1))
        .with(
            "#item_durability_visible",
            Scalar::Bool(slot.durability.is_some()),
        )
        .with("#item_durability_total_amount", Scalar::Num(1000.0))
        .with(
            "#item_durability_current_amount",
            Scalar::Num(slot.durability.unwrap_or(1.0).clamp(0.0, 1.0) * 1000.0),
        )
        .with("#item_storage_visible", Scalar::Bool(false))
}

#[cfg(test)]
mod tests;

fn titles(data: &mut DataSource, model: &HudModel) {
    data.set_global(
        "#hud_title_text_string",
        Scalar::Text(
            model
                .title
                .as_ref()
                .map_or_else(String::new, |title| title.title.clone()),
        ),
    );
    data.set_global(
        "#hud_subtitle_text_string",
        Scalar::Text(
            model
                .title
                .as_ref()
                .map_or_else(String::new, |title| title.subtitle.clone()),
        ),
    );
    if let Some(title) = &model.title {
        data.set_factory(
            "hud_title_text_factory",
            vec![
                FactoryItem::new("hud_title_text", 0.0)
                    .identified(title.creation_id)
                    .clocked(TITLE_CLOCK)
                    .named("hud_title_text")
                    .var("title_fade_in_time", Value::from(title.fade_in))
                    .var("title_stay_time", Value::from(title.stay))
                    .var("title_fade_out_time", Value::from(title.fade_out))
                    .var(
                        "subtitle_initially_visible",
                        Value::Bool(!title.subtitle.is_empty()),
                    )
                    .var("title_alpha", Value::from(title.background_alpha))
                    // The controller drops the shadow once the background shows.
                    .var("title_shadow", Value::Bool(title.background_alpha < 0.5))
                    .var("title_text", Value::from(title.title.clone()))
                    .var("subtitle_text", Value::from(title.subtitle.clone())),
            ],
        );
    }
    if let Some(bar) = &model.actionbar {
        data.set_factory(
            "hud_actionbar_text_factory",
            vec![
                FactoryItem::new("hud_actionbar_text", 0.0)
                    .clocked(ACTIONBAR_CLOCK)
                    .named("hud_actionbar_text")
                    .var("actionbar_text", Value::from(bar.text.clone()))
                    .var(
                        "actionbar_text_background_alpha",
                        Value::from(
                            model
                                .title
                                .as_ref()
                                .map_or(0.0, |title| title.background_alpha),
                        ),
                    ),
            ],
        );
    }
}

fn sidebar(data: &mut DataSource, sidebar: Option<&Sidebar>) {
    let Some(sidebar) = sidebar else {
        return;
    };
    data.set_global(
        "#objective_sidebar_name",
        Scalar::Text(sidebar.title.clone()),
    );
    data.set_global(
        "#scoreboard_sidebar_size",
        Scalar::Num(sidebar.rows.len() as f64),
    );
    data.set_global(
        "#objective_background_opacity",
        Scalar::Num(sidebar.background_opacity),
    );
    data.set_global(
        "#scoreboard_objective_background_opacity",
        Scalar::Num(sidebar.title_background_opacity),
    );
    let text = |name: &str, value: &str| {
        CollectionItem::default().with(name, Scalar::Text(value.to_owned()))
    };
    data.set_collection(
        "scoreboard_players",
        sidebar
            .rows
            .iter()
            .map(|(name, _)| text("#player_name_sidebar", name))
            .collect(),
    );
    data.set_collection(
        "scoreboard_scores",
        sidebar
            .rows
            .iter()
            .map(|(_, score)| text("#player_score_sidebar", score))
            .collect(),
    );
}

/// A `#rrggbb` tint as the `[r, g, b, a]` array a colour binding answers; other
/// text stays text.
fn color_array(color: &str) -> Scalar {
    match crate::emit::color_value(&Value::String(color.to_owned())) {
        Some(rgba) => Scalar::Json(Value::Array(
            rgba.iter()
                .map(|channel| Value::from(f64::from(*channel) / 255.0))
                .collect(),
        )),
        None => Scalar::Text(color.to_owned()),
    }
}

fn boss_bars(data: &mut DataSource, bars: &[BossBar]) {
    data.set_grid_dimensions("#boss_grid_dimension", [1, bars.len() as u32]);
    // Fixed-capacity grids still query unused boss slots.
    data.set_collection_defaults(
        "boss_bars",
        [
            ("#bar_visible".to_owned(), Scalar::Bool(false)),
            ("#bossName".to_owned(), Scalar::Text(String::new())),
        ]
        .into(),
    );
    data.set_collection(
        "boss_bars",
        bars.iter()
            .map(|bar| {
                CollectionItem::default()
                    .with("#bar_visible", Scalar::Bool(true))
                    .with("#bossName", Scalar::Text(bar.name.clone()))
                    .with(
                        "#progress_percentage",
                        Scalar::Num(1.0 - bar.progress.clamp(0.0, 1.0)),
                    )
                    .with("#bar_color", color_array(&bar.color))
                    .with("#bar_notches", Scalar::Num(f64::from(bar.notches)))
            })
            .collect(),
    );
}
