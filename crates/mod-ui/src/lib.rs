//! Presentation of the surfaces mods and server experiences own: trusted consent chrome, a
//! client part's modal, player-mod screens over containers, the personal panel and HUD editor,
//! personal HUD cards and label, and the cosmetic cursor. Screens paint through `ui-engine`
//! from borrowed inputs; the host projects runtime authority into them.

pub mod crosshair;
mod experience;
pub mod experience_modal;
pub mod fonts;
mod mod_hud;
mod mod_hud_editor;
mod mod_panel;
pub use mod_panel::prepared::{PanelUiOwner, PreparedPanelUi};
pub mod mod_screens;
mod mod_widgets;
pub mod template_screen;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use ui::{
    IconRef,
    mod_hud::{Hud, MAX_HUD_ROWS},
};

/// Every mod- and experience-owned surface's state, carried across builds.
#[derive(Default)]
pub struct ModUi {
    mod_hud: Option<mod_hud::ModHud>,
    mod_hud_editor: Option<mod_hud_editor::HudEditor>,
    mod_widgets: Option<mod_widgets::ModWidgets>,
    hud_sources: mod_widgets::sources::HudSources,
    mod_crosshair: Option<ui::mod_hud::Crosshair>,
    mod_panel: Option<mod_panel::ModPanel>,
    prepared_panel_ui: std::collections::BTreeMap<PanelUiOwner, mod_panel::prepared::PreparedOwner>,
    experience: Option<experience::ExperienceChrome>,
    /// A client part's modal screen.
    experience_modal: Option<experience_modal::ModalScreen>,
    /// A player mod's overlay and view.
    mod_screens: Option<mod_screens::ModScreens>,
    /// An explicit embedding override settles mod motion independently of built-in screens.
    reduced_motion: bool,
}

/// Shares the immutable default context used by retained client-owned mod surfaces.
fn personal_context() -> &'static json_ui::Context {
    static CONTEXT: std::sync::OnceLock<json_ui::Context> = std::sync::OnceLock::new();
    CONTEXT.get_or_init(json_ui::Context::default)
}

/// Each personal HUD row's item icon, in card order, as the host's item atlas resolves it.
pub struct RowIcons {
    icons: [Option<IconRef>; MAX_HUD_ROWS],
    count: usize,
}

impl RowIcons {
    /// Resolves every row naming an item through `icon`, by identifier and metadata.
    fn resolve(hud: &Hud, icon: impl Fn(&str, u32) -> Option<IconRef>) -> Self {
        let mut icons = [None; MAX_HUD_ROWS];
        let mut count = 0;
        for row in hud.cards.iter().flat_map(|card| &card.rows) {
            icons[count] = row.item.as_deref().and_then(|id| icon(id, row.metadata));
            count += 1;
        }
        Self { icons, count }
    }

    fn as_slice(&self) -> &[Option<IconRef>] {
        &self.icons[..self.count]
    }
}
