//! The OreUI menu routes as a screen: one paint draws the route over the borrowed menu view,
//! then the adapter draws the player preview it leaves room for, the route's entrance and the
//! character editor.

use std::collections::HashMap;

use ui::{IconRef, UiNode, UiRect};

use super::{
    CharacterPreview, Look, OreUiScreen, add_server, death, dressing_room, friends, home, inbox,
    modal, motion, paint,
    paint::{Canvas, EntranceScope},
    pause, play, profile, progress, scroll_focus, settings, theme, world_settings,
};
use launcher::menu::{
    MenuAction, MenuScreen, MenuView,
    view::{SettingsFocusLandmark, SettingsFocusTarget},
};
use launcher::menu::{MenuField, settings_sections::SettingSection};
use ui_engine::caret::TextSpot;
use ui_engine::engine::screen::{
    PaintError, PaintFrame, Screen, ScreenOutput, ScreenRegistry, Translate,
};
use ui_engine::menu_scroll::MenuScrolls;
use ui_engine::scroll::ScrollArea;

/// Whether this canvas can paint the requested route; presentation owns renderer selection.
pub fn draws_menu(view: &MenuView) -> bool {
    // Out of game, every flow and route is OreUI's, including progress a pending disconnect covers.
    if view.out_of_game() {
        return true;
    }
    let progress = view.connecting || view.local.progress.is_some();
    let covered = view.disconnect_message.is_some();
    !covered
        && (progress
            || view.sign_in_prompt_open()
            || matches!(
                view.screen,
                MenuScreen::Home
                    | MenuScreen::DressingRoom
                    | MenuScreen::Settings
                    | MenuScreen::Profile
                    | MenuScreen::Inbox
                    | MenuScreen::Friends
                    | MenuScreen::Play
                    | MenuScreen::Social
                    | MenuScreen::Servers
                    | MenuScreen::AddServer
                    | MenuScreen::Pause
                    | MenuScreen::Death
            ))
}

/// What one OreUI menu paint reads.
pub struct MenuInput<'a> {
    pub view: &'a MenuView,
    pub size: [f32; 2],
    pub portrait: Option<IconRef>,
    pub translate: Translate<'a>,
    /// Installed and downloaded menu artwork by key.
    pub artwork: &'a HashMap<String, IconRef>,
    /// The title image the home, pause and join screens draw.
    pub title_artwork: Option<IconRef>,
    /// Scroll views a focused control reveals itself in.
    pub scrolls: &'a mut MenuScrolls<MenuAction>,
    /// Last frame's geometry buffers, cleared and refilled.
    pub geometry: MenuGeometry,
}

/// Settings geometry input reads between frames.
#[derive(Default)]
pub struct MenuGeometry {
    pub slider_tracks: Vec<(u16, UiRect, Option<UiRect>)>,
    pub focus_targets: Vec<SettingsFocusTarget>,
    pub focus_landmarks: Vec<SettingsFocusLandmark>,
}

/// The dressing room's character area and the thumbnails it shows.
pub struct DressingPreview {
    pub preview: CharacterPreview,
    pub visible_skins: Vec<usize>,
    pub visible_capes: Vec<usize>,
    pub visible_cosmetics: Vec<usize>,
}

/// What a menu paint leaves its adapter beyond hits and focus.
pub struct MenuPaint {
    pub geometry: MenuGeometry,
    pub scrolls: Vec<ScrollArea>,
    pub spots: Vec<TextSpot<MenuField>>,
    /// The home or pause character, drawn by the adapter under the route's entrance.
    pub character_preview: Option<CharacterPreview>,
    pub dressing_preview: Option<DressingPreview>,
    /// The cosmetic filter button that anchors the category menu.
    pub dressing_filter_anchor: Option<[f32; 4]>,
    /// The settings route took input this frame.
    pub settings_input: bool,
    entrance: Option<EntranceScope>,
    rem: f32,
    surface: motion::Surface,
}

/// Input targets owned by an open dressing room overlay.
pub struct DressingOverlayPaint {
    pub hits: Vec<(MenuAction, UiRect)>,
    pub focus: Vec<MenuAction>,
    pub focus_targets: Vec<SettingsFocusTarget>,
    pub focus_landmarks: Vec<SettingsFocusLandmark>,
    pub spots: Vec<TextSpot<MenuField>>,
}

impl MenuPaint {
    /// Fades the route in over everything drawn since it began, previews included.
    pub fn apply_entrance(&self, nodes: &mut [UiNode], size: [f32; 2]) -> Result<(), PaintError> {
        match self.entrance {
            Some(entrance) => paint::apply_entrance(nodes, entrance, self.rem, size),
            None => Ok(()),
        }
    }
}

fn focus_actions(canvas: &Canvas<'_>) -> Vec<MenuAction> {
    canvas
        .focus_hits
        .iter()
        .map(|(action, _)| *action)
        .collect()
}

impl OreUiScreen {
    /// The route's leftovers from the last paint.
    pub fn take_paint(&mut self) -> Option<MenuPaint> {
        self.paint.take()
    }

    /// Draws the active dressing room overlay above the character preview.
    pub fn paint_dressing_overlay(
        &mut self,
        paint: &MenuPaint,
        view: &MenuView,
        size: [f32; 2],
        artwork: &HashMap<String, IconRef>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<DressingOverlayPaint, PaintError> {
        self.configure_accessibility(&view.settings_options);
        let originals = self
            .originals
            .clone()
            .filter(|_| self.look == Look::Originals);
        let mut editor = Canvas::new(
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            self.accessibility_metrics(frame.metrics),
            frame.solid_page,
            originals.as_deref(),
        );
        editor.appearance = theme::Appearance::from_dark(self.dark_mode);
        editor.artwork = Some(artwork);
        editor.capture_focus = true;
        editor.seconds = frame.now;
        editor.surface = paint.surface;
        editor.transitions = Some(&mut self.transitions);
        dressing_room::draw_overlay(&mut editor, view, size, paint.dressing_filter_anchor)?;
        let focus = focus_actions(&editor);
        Ok(DressingOverlayPaint {
            hits: editor.hits,
            focus,
            focus_targets: editor.focus_targets,
            focus_landmarks: editor.focus_landmarks,
            spots: editor.spots,
        })
    }
}

impl Screen for OreUiScreen {
    type Input<'a> = MenuInput<'a>;
    type Action = MenuAction;

    /// OreUI draws in its own code and adds nothing to the JSON-UI catalog.
    fn register(_registry: &mut ScreenRegistry) {}

    fn paint(
        &mut self,
        input: MenuInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<ScreenOutput<MenuAction>, PaintError> {
        let MenuInput {
            view,
            size,
            portrait,
            translate,
            artwork,
            title_artwork,
            scrolls,
            geometry,
        } = input;
        self.configure_accessibility(&view.settings_options);
        let progress = view.connecting || view.local.progress.is_some();
        let sign_in = view.sign_in_prompt_open();
        let screen = view.screen;
        let originals = self
            .originals
            .clone()
            .filter(|_| self.look == Look::Originals);
        let offsets = scrolls.offsets().clone();
        let seconds = frame.now;
        let mut canvas = Canvas::new(
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            self.accessibility_metrics(frame.metrics),
            frame.solid_page,
            originals.as_deref(),
        );
        self.dark_mode = view.settings_options.oreui_dark_mode();
        canvas.appearance = theme::Appearance::from_dark(self.dark_mode);
        canvas.offsets = offsets;
        canvas.artwork = Some(artwork);
        canvas.title_artwork = title_artwork;
        canvas.seconds = seconds;
        canvas.slider_tracks = geometry.slider_tracks;
        canvas.focus_targets = geometry.focus_targets;
        canvas.focus_landmarks = geometry.focus_landmarks;
        canvas.slider_tracks.clear();
        canvas.focus_targets.clear();
        canvas.focus_landmarks.clear();
        self.transitions.begin_frame(
            view.settings.control_activation,
            view.settings.control_activation_navigation,
            seconds,
        );
        canvas.transitions = Some(&mut self.transitions);
        if screen == MenuScreen::Pause && view.death_presentation.active && !progress {
            death::background(&mut canvas, view.death_presentation, size)?;
        }
        let root_surface = motion::Surface::Screen(match screen {
            MenuScreen::Social | MenuScreen::Servers | MenuScreen::AddServer => MenuScreen::Play,
            screen => screen,
        });
        let root_entrance =
            (screen != MenuScreen::Death).then(|| canvas.begin_entrance(root_surface));
        let motion_rem = canvas.rem;
        let mut dressing_preview = None;
        let mut character_preview = None;
        let mut focus = None;
        let mut settings_input = false;
        if view.out_of_game() && view.flow() == launcher::menu::flow::MenuFlow::Disconnect {
            let error = view
                .disconnect_message
                .as_deref()
                .expect("disconnect flow has a reason");
            canvas.capture_focus = true;
            super::disconnect::draw(&mut canvas, view, size, error, translate)?;
            focus = Some(focus_actions(&canvas));
        } else if sign_in && !progress {
            canvas.capture_focus = true;
            modal::draw(
                &mut canvas,
                view,
                size,
                &super::accounts::sign_in_modal(view),
            )?;
            focus = Some(focus_actions(&canvas));
        } else if view.realm_membership.is_some() && !progress {
            canvas.capture_focus = true;
            super::realm_membership::draw(&mut canvas, view, size)?;
            focus = Some(focus_actions(&canvas));
        } else if progress {
            canvas.capture_focus = true;
            progress::join(&mut canvas, view, size, translate)?;
            focus = Some(focus_actions(&canvas));
        } else {
            match screen {
                MenuScreen::Death => {
                    canvas.capture_focus = true;
                    death::draw(&mut canvas, view, size, translate)?;
                    focus = Some(focus_actions(&canvas));
                }
                MenuScreen::Home => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        character_preview =
                            home::draw(&mut canvas, view, size, portrait, translate)?;
                        if !scroll_focus::reveal(&mut canvas, scrolls, view) || attempt == 1 {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    focus = Some(focus_actions(&canvas));
                }
                MenuScreen::Pause => {
                    canvas.capture_focus = true;
                    character_preview = pause::draw(&mut canvas, view, size, translate)?;
                    focus = Some(focus_actions(&canvas));
                }
                MenuScreen::DressingRoom => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        dressing_preview = Some(dressing_room::draw(&mut canvas, view, size)?);
                        if !scroll_focus::reveal(&mut canvas, scrolls, view) || attempt == 1 {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    focus = Some(focus_actions(&canvas));
                }
                MenuScreen::Profile => profile::draw(&mut canvas, view, size, portrait, artwork)?,
                MenuScreen::Inbox => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        inbox::draw(&mut canvas, view, size, translate)?;
                        if !scroll_focus::reveal(&mut canvas, scrolls, view) || attempt == 1 {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    focus = Some(focus_actions(&canvas));
                }
                MenuScreen::Settings => {
                    let section = SettingSection::from_index(view.settings.section)
                        .unwrap_or(SettingSection::Accessibility);
                    canvas
                        .transitions
                        .as_deref_mut()
                        .unwrap()
                        .begin_settings(section.definition().index);
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        settings::draw(
                            &mut canvas,
                            view,
                            size,
                            translate,
                            artwork.get(&view.feeds.profile.picture_path).copied(),
                            &mut self.shader_option_description,
                        )?;
                        let adjusted = scroll_focus::reveal(&mut canvas, scrolls, view);
                        if !adjusted || attempt == 1 {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                        canvas.slider_tracks.clear();
                    }
                    focus = Some(focus_actions(&canvas));
                    settings_input = true;
                }
                MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers => {
                    match world_settings::route(view.local.screen, &view.local) {
                        Some(route) => {
                            canvas.capture_focus = true;
                            let rollback = (canvas.nodes.len(), *canvas.next);
                            for attempt in 0..2 {
                                let entrance = canvas.begin_entrance(motion::Surface::World(route));
                                world_settings::draw(&mut canvas, view, size, route)?;
                                canvas.end_entrance(entrance, size)?;
                                if !scroll_focus::reveal(&mut canvas, scrolls, view) || attempt == 1
                                {
                                    break;
                                }
                                canvas.nodes.truncate(rollback.0);
                                *canvas.next = rollback.1;
                                canvas.hits.clear();
                                canvas.clear_focus_geometry();
                                canvas.spots.clear();
                                canvas.scrolls.clear();
                            }
                            focus = Some(focus_actions(&canvas));
                        }
                        None => play::draw(&mut canvas, view, size, artwork)?,
                    }
                    if let Some(dialog) = modal::local_world_modal(&view.local) {
                        modal::draw(&mut canvas, view, size, &dialog)?;
                    }
                }
                MenuScreen::AddServer => {
                    play::draw_tab(&mut canvas, view, size, artwork, 2)?;
                    canvas.hits.clear();
                    canvas.clear_focus_geometry();
                    canvas.spots.clear();
                    canvas.scrolls.clear();
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        add_server::draw(&mut canvas, view, size)?;
                        if !scroll_focus::reveal(&mut canvas, scrolls, view) || attempt == 1 {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.spots.clear();
                        canvas.scrolls.clear();
                    }
                    focus = Some(focus_actions(&canvas));
                }
                _ => friends::draw(&mut canvas, view, size)?,
            }
        }
        let hits = canvas.hits;
        self.paint = Some(MenuPaint {
            geometry: MenuGeometry {
                slider_tracks: canvas.slider_tracks,
                focus_targets: canvas.focus_targets,
                focus_landmarks: canvas.focus_landmarks,
            },
            scrolls: canvas.scrolls,
            spots: canvas.spots,
            character_preview,
            dressing_filter_anchor: dressing_preview
                .as_ref()
                .and_then(|area| area.filter_anchor),
            dressing_preview: dressing_preview.map(|area| DressingPreview {
                preview: CharacterPreview {
                    control: area.control,
                    clip: area.clip,
                },
                visible_skins: area.visible_skins,
                visible_capes: area.visible_capes,
                visible_cosmetics: area.visible_cosmetics,
            }),
            settings_input,
            entrance: root_entrance,
            rem: motion_rem,
            surface: root_surface,
        });
        Ok(ScreenOutput {
            hits,
            focus,
            frame: None,
        })
    }
}
