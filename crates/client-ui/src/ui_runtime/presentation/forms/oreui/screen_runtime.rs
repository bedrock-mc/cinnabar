use ui::{UiNode, UiRect};

use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::{
    Look, add_server, death, dressing_room, friends, home, inbox, modal, motion, paint,
    paint::Canvas, pause, play, profile, progress, scroll_focus, settings, theme, world_settings,
};
use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

impl UiPresentationRuntime {
    /// Draws an owned OreUI route, including the owner's menu design extensions.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_oreui_screen(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        portrait: Option<super::super::super::IconRef>,
        translate: super::super::menu_screens::Translate<'_>,
    ) -> Result<Option<Vec<(MenuAction, UiRect)>>, UiPresentationError> {
        // A launcher dialog draws over the OreUI screen instead.
        let progress = view.connecting || view.local.progress.is_some();
        let covered = view.disconnect_message.is_some()
            || matches!(view.auth_state, AuthState::AwaitingCode { .. });
        let screen = view.screen;
        if covered
            || (!progress
                && !matches!(
                    screen,
                    MenuScreen::Home
                        | MenuScreen::Death
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
                ))
        {
            return Ok(None);
        }
        let originals = self
            .form_presentation
            .oreui_originals
            .clone()
            .filter(|_| self.form_presentation.oreui_look == Look::Originals);
        self.menu_scrolls.configure_motion(
            view.settings_options.value("screen_animations") != 0,
            self.menu_seconds,
        );
        let offsets = self.menu_scrolls.offsets().clone();
        let title_artwork = if screen == MenuScreen::Home && !progress {
            self.menu_artwork
                .refs
                .get(super::super::super::menu_artwork::TITLE_KEY)
                .copied()
        } else if progress || screen == MenuScreen::Pause {
            self.form_presentation.engine.as_deref().map_or_else(
                || {
                    self.menu_artwork
                        .refs
                        .get(super::super::super::menu_artwork::TITLE_KEY)
                        .copied()
                },
                |engine| engine.menu_title(&self.menu_artwork.refs),
            )
        } else {
            None
        };
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            originals.as_deref(),
        );
        self.form_presentation.oreui_dark_mode = view.settings_options.oreui_dark_mode();
        canvas.appearance = theme::Appearance::from_dark(self.form_presentation.oreui_dark_mode);
        canvas.offsets = offsets;
        canvas.artwork = Some(&self.menu_artwork.refs);
        canvas.title_artwork = title_artwork;
        canvas.seconds = self.menu_seconds;
        canvas.slider_tracks = std::mem::take(&mut self.form_presentation.oreui_slider_tracks);
        canvas.focus_targets = std::mem::take(&mut self.form_presentation.menu_focus_geometry);
        canvas.focus_landmarks = std::mem::take(&mut self.form_presentation.menu_focus_landmarks);
        canvas.slider_tracks.clear();
        canvas.focus_targets.clear();
        canvas.focus_landmarks.clear();
        self.form_presentation.oreui_transitions.begin_frame(
            view.settings_control_activation,
            view.settings_control_activation_navigation,
            self.menu_seconds,
        );
        canvas.transitions = Some(&mut self.form_presentation.oreui_transitions);
        let root_surface = motion::Surface::Screen(match screen {
            MenuScreen::Social | MenuScreen::Servers | MenuScreen::AddServer => MenuScreen::Play,
            screen => screen,
        });
        let root_entrance = canvas.begin_entrance(root_surface);
        let motion_rem = canvas.rem;
        let mut dressing_preview = None;
        let mut character_preview = None;
        if progress {
            canvas.capture_focus = true;
            progress::join(&mut canvas, view, size, translate)?;
            self.form_presentation.menu_focus = canvas
                .focus_hits
                .iter()
                .map(|(action, _)| *action)
                .collect();
        } else {
            match screen {
                MenuScreen::Home => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        character_preview =
                            home::draw(&mut canvas, view, size, portrait, translate)?;
                        if !scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
                            || attempt == 1
                        {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                }
                MenuScreen::Pause => {
                    canvas.capture_focus = true;
                    character_preview = pause::draw(&mut canvas, view, size)?;
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                }
                MenuScreen::DressingRoom => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        dressing_preview = Some(dressing_room::draw(&mut canvas, view, size)?);
                        if !scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
                            || attempt == 1
                        {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                }
                MenuScreen::Death => {
                    canvas.appearance =
                        theme::Appearance::from_dark(self.form_presentation.oreui_dark_mode);
                    canvas.bundle = theme::Bundle::Gameplay;
                    death::draw(&mut canvas, view, size)?
                }
                MenuScreen::Profile => {
                    profile::draw(&mut canvas, view, size, portrait, &self.menu_artwork.refs)?
                }
                MenuScreen::Inbox => {
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        inbox::draw(&mut canvas, view, size, translate)?;
                        if !scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
                            || attempt == 1
                        {
                            break;
                        }
                        canvas.nodes.truncate(rollback.0);
                        *canvas.next = rollback.1;
                        canvas.hits.clear();
                        canvas.clear_focus_geometry();
                        canvas.scrolls.clear();
                    }
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                }
                MenuScreen::Settings => {
                    let section = super::super::menu_screens::SETTINGS_SECTIONS
                        .iter()
                        .find_map(|(key, index)| (*index == view.settings_section).then_some(*key))
                        .unwrap_or("accessibility_forced_index");
                    canvas
                        .transitions
                        .as_deref_mut()
                        .unwrap()
                        .begin_settings(settings::section_index(section));
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        settings::draw(
                            &mut canvas,
                            view,
                            size,
                            translate,
                            self.menu_artwork
                                .refs
                                .get(&view.feeds.profile.picture_path)
                                .copied(),
                        )?;
                        let adjusted =
                            scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view);
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
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                    self.settings_slider_drag_targets.clear();
                    self.form_presentation.oreui_settings_input = true;
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
                                if !scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
                                    || attempt == 1
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
                            self.form_presentation.menu_focus = canvas
                                .focus_hits
                                .iter()
                                .map(|(action, _)| *action)
                                .collect();
                        }
                        None if screen == MenuScreen::Servers => {
                            canvas.capture_focus = true;
                            let rollback = (canvas.nodes.len(), *canvas.next);
                            for attempt in 0..2 {
                                play::draw(&mut canvas, view, size, &self.menu_artwork.refs)?;
                                if view.dialog == Some(crate::menu::MenuDialog::ServerFilter)
                                    || !scroll_focus::reveal(
                                        &mut canvas,
                                        &mut self.menu_scrolls,
                                        view,
                                    )
                                    || attempt == 1
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
                            self.form_presentation.menu_focus = canvas
                                .focus_hits
                                .iter()
                                .map(|(action, _)| *action)
                                .collect();
                        }
                        None => play::draw(&mut canvas, view, size, &self.menu_artwork.refs)?,
                    }
                    if let Some(dialog) = modal::local_world_modal(&view.local) {
                        modal::draw(&mut canvas, view, size, &dialog)?;
                    }
                }
                MenuScreen::AddServer => {
                    play::draw_tab(&mut canvas, view, size, &self.menu_artwork.refs, 2)?;
                    canvas.hits.clear();
                    canvas.clear_focus_geometry();
                    canvas.spots.clear();
                    canvas.scrolls.clear();
                    canvas.capture_focus = true;
                    let rollback = (canvas.nodes.len(), *canvas.next);
                    for attempt in 0..2 {
                        add_server::draw(&mut canvas, view, size)?;
                        if !scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
                            || attempt == 1
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
                    self.form_presentation.menu_focus = canvas
                        .focus_hits
                        .iter()
                        .map(|(action, _)| *action)
                        .collect();
                }
                _ => friends::draw(&mut canvas, view, size)?,
            }
        }
        let (mut hits, scrolls, spots, slider_tracks, focus_targets, focus_landmarks) = (
            canvas.hits,
            canvas.scrolls,
            canvas.spots,
            canvas.slider_tracks,
            canvas.focus_targets,
            canvas.focus_landmarks,
        );
        self.form_presentation.oreui_slider_tracks = slider_tracks;
        self.form_presentation.menu_focus_geometry = focus_targets;
        self.form_presentation.menu_focus_landmarks = focus_landmarks;
        if view.dialog != Some(crate::menu::MenuDialog::ServerFilter) {
            self.menu_scrolls.set_areas(scrolls);
        }
        self.add_menu_text_spots(spots);
        if let Some(preview) = character_preview {
            self.append_menu_player_preview(
                nodes,
                next,
                metrics,
                preview.control,
                preview.clip,
                super::super::super::player_preview::MenuPreviewConfig::DRESSING_ROOM,
            )?;
        }
        if let Some(preview) = dressing_preview {
            self.menu_skin_thumbnail_indices = preview.visible_skins;
            self.menu_cape_thumbnail_indices = preview.visible_capes;
            self.append_menu_player_preview(
                nodes,
                next,
                metrics,
                preview.control,
                preview.clip,
                super::super::super::player_preview::MenuPreviewConfig {
                    starting_rotation: if view.dressing_room.section
                        == launcher::dressing_room::DressingRoomSection::Capes
                    {
                        210.0
                    } else {
                        30.0
                    },
                    ..super::super::super::player_preview::MenuPreviewConfig::DRESSING_ROOM
                },
            )?;
            paint::apply_entrance(nodes, root_entrance, motion_rem, size)?;
            if view.dressing_room.editor.is_some() {
                self.menu_preview.control = None;
                self.cancel_menu_player_preview_input();
                let (editor_hits, focus, targets, landmarks, spots) = {
                    let mut editor = Canvas::new(
                        nodes,
                        next,
                        &mut self.layouts,
                        &self.font,
                        metrics,
                        self.solid_texture_page,
                        originals.as_deref(),
                    );
                    editor.appearance =
                        theme::Appearance::from_dark(self.form_presentation.oreui_dark_mode);
                    editor.artwork = Some(&self.menu_artwork.refs);
                    editor.capture_focus = true;
                    editor.seconds = self.menu_seconds;
                    editor.surface = root_surface;
                    editor.transitions = Some(&mut self.form_presentation.oreui_transitions);
                    dressing_room::draw_editor(&mut editor, view, size)?;
                    (
                        editor.hits,
                        editor
                            .focus_hits
                            .iter()
                            .map(|(action, _)| *action)
                            .collect(),
                        editor.focus_targets,
                        editor.focus_landmarks,
                        editor.spots,
                    )
                };
                self.form_presentation.menu_focus = focus;
                self.form_presentation.menu_focus_geometry = targets;
                self.form_presentation.menu_focus_landmarks = landmarks;
                hits = editor_hits;
                self.add_menu_text_spots(spots);
                self.menu_scrolls.set_areas(Vec::new());
            }
        } else {
            paint::apply_entrance(nodes, root_entrance, motion_rem, size)?;
        }
        self.form_presentation.menu_sounds.clear();
        Ok(Some(hits))
    }
}
