//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod chat;
mod chat_settings;
mod collection_reveal;
#[cfg(feature = "test-support")]
mod cosmetics;
mod hud_card_fit;
mod hud_cards;
mod hud_editor;
mod local_identity;
mod minecraft_import;
mod mods;
mod out_of_game;
mod roster;
mod shader_options;
mod shader_pack;
mod states;
mod updates;

use std::sync::Arc;

use launcher::menu::{
    MenuAction, MenuScreen, MenuView, settings_options::SettingId,
    settings_sections::SettingSection,
};
use ui_screens::player_list::RosterRow;

/// A repeatable viewport and the runtime observations its screen paints.
pub struct State {
    pub name: &'static str,
    pub size: [u32; 2],
    pub gui_scale: u8,
    pub fixture: fn() -> Fixture,
}

/// Inputs borrowed by the same screen painters the client uses.
pub enum Fixture {
    /// Unequal mask edges and high-resolution sprite minification.
    MaskRegression,
    /// Original sprite samples before and after alpha-only outline shading.
    SpriteOutline {
        outline: bool,
    },
    /// Adjacent sparse saturation masks before and after overlap-aware exclusion.
    SaturationOverlap {
        crop: bool,
        source_alpha: u8,
    },
    /// A label whose unavailable primary face selects its installed backup.
    BackupFont,
    /// An original checker repeated beyond a small sprite's former draw cutoff.
    TiledImage {
        scale: f64,
    },
    /// A server form whose editable controls are locked during a session.
    #[cfg(feature = "test-support")]
    ControlLocked,
    /// An equipped original cosmetic rendered by the full production particle-preview path.
    #[cfg(feature = "test-support")]
    CosmeticParticles {
        view: client_ui::ui_runtime::presentation::player_preview::PreviewView,
        emitting: bool,
    },
    Menu(Box<MenuView>),
    Loading(ui::LoadingStage),
    Chat {
        draft: &'static str,
        suggestions: Vec<Arc<str>>,
        messages: std::collections::VecDeque<ui::ChatMessage>,
        options: Arc<launcher::menu::settings_options::SettingsOptions>,
        server_hover: Option<bool>,
    },
    /// The held tab roster's rows.
    TabList(Vec<RosterRow>),
    /// Two original custom-form inputs after selection moves from the first to the second.
    EditSelection,
    /// An externally supplied bounded native mod panel.
    ModPanel(
        Box<ui::mod_panel::Panel>,
        Option<Box<ui::mod_hud::Hud>>,
        Option<(&'static str, render_model::UiTexturePage)>,
    ),
    /// A selected HUD card before or after retained placement and visibility edits.
    HudEditor {
        hud: Box<ui::mod_hud::Hud>,
        edited: bool,
    },
    /// Two same-ID requests arriving while the first menu is still open.
    #[cfg(feature = "test-support")]
    FormOverlap,
    /// An edited settings draft after switching away from and back to its tab.
    #[cfg(feature = "test-support")]
    SettingsDraft,
    /// A damaged stack picked up from the survival inventory.
    #[cfg(feature = "test-support")]
    CursorDurability,
    /// Survival recipe-book inputs admitted by the inventory session and wire decoder.
    #[cfg(feature = "test-support")]
    CraftOverlap {
        selected: bool,
    },
    /// A client toast as the toast screen shows it once slid in.
    Toast {
        title: String,
        message: String,
    },
}

const SIZE: [u32; 2] = [1280, 720];
const GUI_SCALE: u8 = 2;
const SMALL_SIZE: [u32; 2] = [360, 240];
/// Fits an expanded card above two collapsed ones.
const TALL_SIZE: [u32; 2] = [1280, 900];
/// Keeps the first hotbar rows visible below the keyboard options.
const KEYBOARD_SIZE: [u32; 2] = [SIZE[0], 2000];
/// Reaches the Video pane's Renderer row below the general options.
const SHADER_PACK_ROW_SIZE: [u32; 2] = [SIZE[0], 1600];

/// A capture at GUI scale 1 that needs no game assets.
const fn asset_free(name: &'static str, size: [u32; 2], fixture: fn() -> Fixture) -> State {
    State {
        name,
        size,
        gui_scale: 1,
        fixture,
    }
}

/// Opens a settings category by its registered name rather than a copied index.
pub fn settings(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings.section = launcher::menu::settings_sections::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("registered settings category");
    view
}

/// Shows the keyboard options and command macro modifier without a live session.
fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}

/// Scrolls the General section to toggle `id` through the client's focus scrolling; a toggle's
/// action carries the value it switches to.
fn settings_general_toggle(id: SettingId) -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    view.focused_action = Some(MenuAction::SettingsOption(
        id.index() as u16,
        1 - view.settings_options.value(id),
    ));
    Fixture::Menu(Box::new(view))
}

/// The Advanced rows with Verbose logging and Record network session turned on.
fn settings_general_diagnostics_on() -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    let mut options = (*view.settings_options).clone();
    for id in [SettingId::VerboseLogging, SettingId::RecordNetworkSession] {
        options.set(id.index(), 1);
    }
    view.settings_options = Arc::new(options);
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::RecordNetworkSession.index() as u16,
        0,
    ));
    Fixture::Menu(Box::new(view))
}

/// Shows the same physical hotbar binding with a fallback or an observed AZERTY label.
fn settings_keyboard_layout(observed: bool) -> Fixture {
    use launcher::menu::settings_options::{
        BindingId, KEY_BINDINGS, control_bindings::decode_control,
    };

    let mut view = settings("keyboard_and_mouse_forced_index");
    let row = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == BindingId::Hotbar1.name())
        .unwrap();
    let settings = Arc::make_mut(&mut view.settings_options);
    assert!(settings.remap(row, decode_control(0x14).unwrap()));
    if observed {
        settings.set_key_label(0x14, "a");
    }
    Fixture::Menu(Box::new(view))
}

/// Enumerates every fixture, including the Chat section's independently owned states.
pub fn states() -> impl Iterator<Item = &'static State> {
    states::STATES.iter().chain(chat_settings::STATES)
}
