//! A player mod's screens beside the vanilla container screens: its overlay, laid out over the
//! whole root but drawn and hit only outside the container's panels and exclusion areas, and
//! its view, drawn over the still-open container while vanilla's screen is hidden. Both follow
//! the package screen rules of [`super::template_screen`]; the vanilla held stack and tooltips
//! stay on top of them, and toasts and trusted chrome draw later still.

use std::{collections::BTreeSet, sync::Arc};

use server_experience::screen::{self, GuiSize, ScreenLayout};
use ui::UiNode;

use super::super::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationRuntime};
use super::{
    engine::{EngineInputs, ScreenArt},
    template_screen::{ModalEdits, TemplateArt, TemplateScreen},
};
use crate::ui_runtime::UiRuntime;

mod layering;

use layering::Surface;

/// What a player mod draws, as its host committed it.
pub struct ModScreensInput<'a> {
    /// The mod's id, which owns its catalog and textures.
    pub id: &'a str,
    pub files: &'a Arc<screen::Files>,
    pub overlay: Option<&'a String>,
    pub view: Option<&'a String>,
    /// The last `focus-text` request and the data revision it was made at.
    pub focus: Option<&'a (u64, String)>,
    pub data: &'a screen::Modal,
}

pub(super) struct ModScreens {
    art: TemplateArt,
    overlay: TemplateScreen,
    view: TemplateScreen,
    /// The revision of the last focus request applied.
    focus_applied: u64,
    /// Selection edits waiting for the mod adapter after vanilla input routing.
    pending_edits: ModalEdits,
    /// Every `#item_id_aux` key the bound data names, for the item icons.
    icon_keys: BTreeSet<i64>,
    /// The container screen this build drew, as the mod sees it.
    layout: Option<ScreenLayout>,
}

impl ModScreens {
    /// Forgets the last build's drawing: a build that draws no container screen leaves the mod
    /// with no layout and nothing drawn.
    pub(super) fn begin_frame(&mut self) {
        self.layout = None;
        self.overlay.frame = None;
        self.view.frame = None;
    }

    /// Gives overlay controls the pointer ahead of the view's full-screen dismiss regions.
    fn pointer_points(&self, point: Option<[f32; 2]>) -> (Option<[f32; 2]>, Option<[f32; 2]>) {
        if point.is_some_and(|point| self.overlay.control_at(point).is_some()) {
            (None, point)
        } else {
            (point, None)
        }
    }

    /// The screen under window-logical `point`; see [`layering::pointer_surface`].
    fn surface_at(&mut self, point: [f32; 2]) -> Option<&mut TemplateScreen> {
        let view_open = self.view.frame.is_some();
        let frame = match &self.view.frame {
            Some(frame) => frame,
            None => self.overlay.frame.as_ref()?,
        };
        let gui = layering::to_gui(frame, point);
        Some(
            match layering::pointer_surface(view_open, self.layout.as_ref(), gui) {
                Surface::View => &mut self.view,
                Surface::Overlay => &mut self.overlay,
            },
        )
    }
}

impl UiPresentationRuntime {
    /// Follows the player mod's committed screens; `None` (no mod, or a quarantined one)
    /// removes them and drops their catalog and textures.
    pub fn set_mod_screens(&mut self, input: Option<ModScreensInput<'_>>) {
        let page =
            (self.textures.dynamic_start() + super::super::dynamic_textures::MOD_UI_PAGE) as u16;
        let slot = &mut self.form_presentation.mod_screens;
        let Some(input) = input else {
            *slot = None;
            return;
        };
        if slot
            .as_ref()
            .is_none_or(|current| !current.art.is(input.id, input.files))
        {
            // Keep the last built container layout; this replacement has no drawn view yet.
            let layout = slot
                .as_ref()
                .and_then(|current| current.layout.clone())
                .map(|mut layout| {
                    layout.view = None;
                    layout
                });
            *slot = Some(ModScreens {
                art: TemplateArt::new(
                    input.id,
                    input.files,
                    page,
                    super::super::dynamic_textures::MOD_UI_PAGES,
                ),
                overlay: TemplateScreen::default(),
                view: TemplateScreen::default(),
                focus_applied: 0,
                pending_edits: ModalEdits::default(),
                icon_keys: BTreeSet::new(),
                layout,
            });
        }
        let screens = slot.as_mut().expect("mod screens installed");
        screens.overlay.follow(input.overlay, input.data);
        screens.view.follow(input.view, input.data);
        screens.icon_keys = icon_keys(input.data);
        if let Some((revision, control)) = input.focus
            && *revision > screens.focus_applied
        {
            screens.focus_applied = *revision;
            // The box may be on either screen; each selects only its own box by that name.
            screens.view.focus(control);
            screens.overlay.focus(control);
        }
    }

    /// The container screen the last build drew, as the mod's `screen.layout` reports it.
    pub fn mod_screen_layout(&self) -> Option<&ScreenLayout> {
        self.form_presentation.mod_screens.as_ref()?.layout.as_ref()
    }

    /// Whether the retained presentation belongs to this package snapshot.
    pub fn mod_screens_match(&self, id: &str, files: &Arc<screen::Files>) -> bool {
        self.form_presentation
            .mod_screens
            .as_ref()
            .is_some_and(|screens| screens.art.is(id, files))
    }

    /// Why the mod's templates were refused, which quarantines the mod.
    pub fn mod_screens_failure(&self) -> Option<&str> {
        self.form_presentation.mod_screens.as_ref()?.art.failure()
    }

    /// Whether the last build drew the mod's view, which then owns pointer and keyboard and
    /// hides the container screen.
    pub fn mod_view_shown(&self) -> bool {
        self.form_presentation
            .mod_screens
            .as_ref()
            .is_some_and(|screens| screens.view.frame.is_some())
    }

    /// Whether one of the mod's edit boxes is selected and takes typing.
    pub fn mod_text_focused(&self) -> bool {
        self.form_presentation
            .mod_screens
            .as_ref()
            .is_some_and(|screens| screens.view.text_focused() || screens.overlay.text_focused())
    }

    /// Whether window-logical `point` is the mod's: anywhere while the view is shown, else on an
    /// overlay control (which is never inside the container's panels). A press there is not
    /// vanilla's and never drops the held stack.
    pub fn mod_screens_own(&self, point: [f32; 2]) -> bool {
        let Some(screens) = self.form_presentation.mod_screens.as_ref() else {
            return false;
        };
        screens.view.frame.is_some() || screens.overlay.control_at(point).is_some()
    }

    /// The collection name and index of the mod's control under window-logical `point`, on the
    /// screen the pointer is over (the overlay beside an open view); what a declared key
    /// reports as its row.
    pub fn mod_screens_row(&mut self, point: [f32; 2]) -> Option<(String, u32)> {
        let screens = self.form_presentation.mod_screens.as_mut()?;
        let frame = screens.surface_at(point)?.frame.as_ref()?;
        let regions = frame.hits.iter().map(|hit| {
            let row = hit.collection.as_deref().zip(hit.collection_index);
            (
                [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h],
                row,
                hit.enabled,
            )
        });
        layering::nearest_row(regions, layering::to_gui(frame, point))
    }

    /// Routes hover and wheel positions to the drawn surface under the pointer.
    pub fn hover_mod_screens(&mut self, point: Option<[f32; 2]>) {
        let Some(screens) = self.form_presentation.mod_screens.as_mut() else {
            return;
        };
        screens.view.hover(None);
        screens.overlay.hover(None);
        if let Some(point) = point
            && let Some(screen) = screens.surface_at(point)
        {
            screen.hover(Some(point));
        }
    }

    /// Wheel `notches` (positive scrolls down) at window-logical `point`: a scroll view under it
    /// takes them, else they are the mod's at the returned GUI point when the pointer is over
    /// the view or outside the container's panels and exclusions.
    pub fn scroll_mod_screens(&mut self, point: [f32; 2], notches: f64) -> Option<[f64; 2]> {
        let screens = self.form_presentation.mod_screens.as_mut()?;
        if notches == 0.0 {
            return None;
        }
        let view_open = screens.view.frame.is_some();
        let layout = screens.layout.clone();
        let screen = screens.surface_at(point)?;
        let gui = layering::to_gui(screen.frame.as_ref()?, point);
        let allowed = view_open || layout.is_some_and(|layout| layout.overlay_allows(gui));
        if !allowed || screen.scroll(notches) {
            return None;
        }
        Some(gui)
    }

    /// Tracks a left press on its owning screen; see [`TemplateScreen::press`].
    pub fn press_mod_screens(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let screens = self.form_presentation.mod_screens.as_mut()?;
        let (view, overlay) = screens.pointer_points(point);
        let view = screens.view.press(view, pressed, released);
        let overlay = screens.overlay.press(overlay, pressed, released);
        view.or(overlay)
    }

    /// Tracks a right press on the view and the overlay beside it; see
    /// [`TemplateScreen::secondary_press`].
    pub fn secondary_press_mod_screens(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let screens = self.form_presentation.mod_screens.as_mut()?;
        let (view, overlay) = screens.pointer_points(point);
        let view = screens.view.secondary_press(view, pressed, released);
        let overlay = screens.overlay.secondary_press(overlay, pressed, released);
        view.or(overlay)
    }

    /// Applies pointer selection before vanilla shortcuts and retains edits for the mod adapter.
    pub fn focus_mod_screen_pointer(&mut self, point: Option<[f32; 2]>, now: f64) {
        let edits = self.edit_mod_screens(point, true, &[], false, now);
        if let Some(screens) = self.form_presentation.mod_screens.as_mut() {
            screens.pending_edits = edits;
        }
    }

    /// Drives the edit boxes of the view and the overlay beside it: a press selects the box
    /// under it and deselects the other screen's, typing goes to the selected box, and Escape
    /// deselects; see [`TemplateScreen::edit`].
    pub fn edit_mod_screens(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        typed: &[String],
        escape: bool,
        now: f64,
    ) -> ModalEdits {
        let Some(screens) = self.form_presentation.mod_screens.as_mut() else {
            return ModalEdits::default();
        };
        let (view, overlay) = screens.pointer_points(point);
        let mut edits = screens.view.edit(view, pressed, typed, escape, now);
        let overlay = screens.overlay.edit(overlay, pressed, typed, escape, now);
        edits.edits.extend(overlay.edits);
        edits.escape_consumed |= overlay.escape_consumed;
        let mut pending = std::mem::take(&mut screens.pending_edits);
        pending.edits.extend(edits.edits);
        pending.escape_consumed |= edits.escape_consumed;
        pending
    }

    /// Draws the mod's view and overlay over the container screen this build drew, from node
    /// `container_start` on; the container's held stack and tooltips move above them.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_mod_screens(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        container_start: usize,
    ) {
        let Some(keys) = self
            .form_presentation
            .mod_screens
            .as_ref()
            .map(|screens| screens.icon_keys.clone())
        else {
            return;
        };
        let id_aux = item_icons(&keys, player_runtime, runtime, |id, aux| {
            self.item_icon(id, aux)
        });
        let screens = self
            .form_presentation
            .mod_screens
            .as_mut()
            .expect("mod screens checked above");
        screens.begin_frame();
        let (Some(renderer), Some((frame, _))) = (
            self.form_presentation.engine.as_deref(),
            self.form_presentation.container.as_ref(),
        ) else {
            return;
        };
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let root = [f64::from(content[0] / px), f64::from(content[1] / px)];
        let Some(gui) = layering::gui_rect(
            frame.panel,
            frame
                .hits
                .iter()
                .map(|hit| [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h]),
            root,
        ) else {
            return;
        };
        let Some(reference) = super::container_screen_reference(player_runtime, runtime) else {
            return;
        };
        let mut layout = ScreenLayout {
            screen: reference.to_owned(),
            size: GuiSize {
                width: root[0],
                height: root[1],
                scale: f64::from(px),
            },
            gui,
            exclusions: Vec::new(),
            view: None,
        };
        let lifted = layering::take_lifted(nodes, &frame.top);
        let translate = |key: &str| runtime.translation(key);
        let font = &self.font;
        let (solid_page, safe_area) = (self.solid_texture_page, self.safe_area);
        let layouts = &mut self.layouts;
        let art = || ScreenArt {
            id_aux: &id_aux,
            ..ScreenArt::default()
        };
        macro_rules! inputs {
            () => {
                EngineInputs {
                    layouts: &mut *layouts,
                    font,
                    metrics,
                    solid_page,
                    safe_area,
                    content,
                    translate: &translate,
                    language: runtime.text_generation(),
                }
            };
        }
        let mut view_tooltips = None;
        if screens.view.template.is_some() {
            let view_start = nodes.len();
            if screens
                .view
                .draw(&mut screens.art, renderer, inputs!(), (nodes, next), art())
            {
                let hits = Arc::clone(&screens.view.frame.as_ref().expect("drawn view").hits);
                screens.view.finish_input(&hits);
                // The view's tooltips draw above the overlay and never widen what it keeps
                // clear of.
                if let Some(view) = screens.view.frame.as_ref() {
                    view_tooltips = Some(layering::take_lifted(nodes, &view.top));
                }
                // The view stands in for the container screen, which stays open underneath,
                // and for its panels as what the overlay keeps clear of.
                layout.view = layering::drawn_bounds(&nodes[view_start..], content, px);
                nodes.drain(container_start..view_start);
            }
        }
        let start = nodes.len();
        if screens
            .overlay
            .draw(&mut screens.art, renderer, inputs!(), (nodes, next), art())
            && let Some(overlay) = screens.overlay.frame.as_mut()
        {
            let forbidden = layering::forbidden_logical(&layout, overlay.scale);
            layering::clip_overlay(nodes, start, &forbidden, &overlay.top, next);
            let hits = Arc::clone(&overlay.hits);
            layering::keep_allowed_hits(overlay, &layout);
            screens.overlay.finish_input(&hits);
        }
        if let Some(view_tooltips) = view_tooltips {
            layering::restore_lifted(nodes, view_tooltips, next);
        }
        layering::restore_lifted(nodes, lifted, next);
        screens.layout = Some(layout);
    }

    /// Copies the mod atlas's page images when they changed; `true` asks for a page rebuild.
    pub(super) fn refresh_mod_screen_pages(&mut self) -> bool {
        self.form_presentation
            .mod_screens
            .as_mut()
            .is_some_and(|screens| screens.art.refresh_pages())
    }

    /// The mod atlas's pages, for the dynamic pages reserved to it.
    pub(in super::super) fn mod_screen_pages(&self) -> &[render_model::UiTexturePage] {
        self.form_presentation
            .mod_screens
            .as_ref()
            .map_or(&[], |screens| screens.art.pages())
    }
}

/// Every `#item_id_aux` integer the bound values and rows name.
fn icon_keys(data: &screen::Modal) -> BTreeSet<i64> {
    let key = |name: &String, value: &screen::Value| match value {
        screen::Value::Integer(key) if name == "#item_id_aux" => Some(*key),
        _ => None,
    };
    data.values
        .iter()
        .filter_map(|(name, value)| key(name, value))
        .chain(
            data.collections
                .values()
                .flat_map(|rows| rows.iter())
                .flat_map(|row| row.iter())
                .filter_map(|(name, value)| key(name, value)),
        )
        .collect()
}

/// Each key's icon: vanilla's `#item_id_aux` is the network id over 16 bits of aux, which the
/// session's item registry names.
fn item_icons(
    keys: &BTreeSet<i64>,
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&str, u32) -> Option<crate::ui_runtime::presentation::IconRef>,
) -> Vec<(i64, crate::ui_runtime::presentation::IconRef)> {
    let Some(registry) = runtime
        .inventory_ledger(player_runtime)
        .negotiated_item_registry()
    else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| {
            let network_id = i32::try_from(key >> 16).ok()?;
            let aux = u32::try_from(key & 0xffff).ok()?;
            let entry = registry.get(&network_id)?;
            Some((*key, icon(&entry.identifier, aux)?))
        })
        .collect()
}

#[cfg(test)]
mod tests;
