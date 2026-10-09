//! Scene painting and retained frame publication.

use super::*;
use crate::ui_runtime::render_adapter;
use ui::RetainedDraw;

impl UiPresentationRuntime {
    /// The Java-look surfaces outside the engine HUD, over the safe HUD geometry.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    fn append_java_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next_id: &mut u32,
        geometry: Option<HudGeometry>,
        now_millis: u64,
        container: bool,
    ) -> Result<(), UiPresentationError> {
        let (Some(hud_textures), Some(geometry)) = (self.hud_textures.as_ref(), geometry) else {
            return Ok(());
        };
        let mut frame = self.hud_frame.clone();
        frame.now_millis = now_millis;
        HudLayout::new(
            nodes,
            next_id,
            hud_textures,
            &mut self.layouts,
            &self.font,
            self.solid_texture_page,
            geometry,
        )?
        .append(player_runtime, runtime, &frame, container)
    }

    /// Builds the frame from its retained UI authority.
    pub fn build(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        now_millis: u64,
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
    ) -> Result<UiRenderInput, UiPresentationError> {
        #[cfg(feature = "tracy")]
        let _build_span = bevy::log::info_span!("ui.build").entered();
        dynamic_textures::observe_session(self, runtime.session_id());
        session_icons::observe(self, runtime.session_icons());
        self.observe_server_ui(runtime.server_ui());
        session_glyphs::observe(self, runtime.session_glyphs());
        // Install artwork before any screen resolves its pixel UVs.
        if self.menu_artwork_loader.poll() {
            self.rebuild_dynamic_textures();
        }
        self.menu_seconds = now_millis as f64 / 1_000.0;
        self.configure_oreui_motion();
        if let Some(view) = &self.menu_view {
            self.menu_scrolls.configure_motion(
                view.settings_options.value("screen_animations") != 0,
                self.menu_seconds,
            );
        }
        let frame = (physical_size, dpi_scale.get(), self.safe_area);
        if let Some(input) = self.retained_menu_input(runtime, frame) {
            return Ok(input);
        }
        self.retained_menu = None;
        let logical_width = physical_size[0] as f32 / dpi_scale.get();
        let logical_height = physical_size[1] as f32 / dpi_scale.get();
        let metrics =
            TextMetrics::for_viewport(physical_size, dpi_scale, self.gui_scale_preference);
        // The gameplay HUD lays out in Java GUI pixels; it fails closed to no
        // HUD when the safe viewport cannot contain the fixed-width hotbar.
        let safe_area = self.safe_area;
        let hud_geometry = self.hud_textures.as_ref().and_then(|_| {
            HudGeometry::new(
                physical_size,
                dpi_scale.get(),
                safe_area,
                self.gui_scale_preference,
            )
        });
        let viewport = rect(0.0, 0.0, logical_width, logical_height)?;
        // Root nodes lay out relative to the safe content rect; the retained
        // tree translates them by the safe-area origin.
        let content_width = (logical_width - safe_area.left() - safe_area.right()).max(0.0);
        let content_height = (logical_height - safe_area.top() - safe_area.bottom()).max(0.0);
        let mut nodes = std::mem::take(&mut self.assembly_nodes);
        nodes.clear();
        let result = (|| {
            let mut next_id = 1u32;
            let content = [content_width, content_height];
            let host = self.scene_host();
            let stack = runtime.scenes_in(player_runtime, host, &self.screen_settings());
            let scenes = stack.visible(false);
            self.begin_form_frame();
            let open: Vec<Scene> = stack.scenes().iter().map(|scene| scene.key).collect();
            self.scene_clocks.observe(&open, self.menu_seconds);
            let mut menu_hit_targets = Vec::new();
            for scene in &scenes {
                self.scene_clock = self.scene_clocks.clocks(*scene);
                let (nodes, next) = (&mut nodes, &mut next_id);
                match scene {
                    Scene::Gameplay => {
                        self.append_java_hud(
                            player_runtime,
                            runtime,
                            nodes,
                            next,
                            hud_geometry,
                            now_millis,
                            false,
                        )?;
                    }
                    Scene::Crosshair | Scene::Hud => {
                        let crosshair = *scene == Scene::Crosshair;
                        self.append_engine_hud(
                            player_runtime,
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content,
                            now_millis,
                            crosshair,
                        )?;
                        if !crosshair {
                            self.append_mod_hud(
                                player_runtime,
                                runtime,
                                nodes,
                                next,
                                metrics,
                                content,
                            );
                            self.append_mod_widgets(
                                player_runtime,
                                runtime,
                                nodes,
                                next,
                                metrics,
                                content,
                            );
                            self.append_player_list(
                                player_runtime,
                                runtime,
                                nodes,
                                next,
                                metrics,
                                content,
                            )?;
                        }
                    }
                    Scene::Bed => {
                        self.append_bed_screen(runtime, nodes, next, metrics, content, now_millis)?;
                    }
                    Scene::Container => {
                        self.append_java_hud(
                            player_runtime,
                            runtime,
                            nodes,
                            next,
                            hud_geometry,
                            now_millis,
                            true,
                        )?;
                        self.append_container_scene(
                            player_runtime,
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content_width,
                            content_height,
                        )?;
                    }
                    Scene::Chat => {
                        self.append_chat_screen(
                            runtime, nodes, next, metrics, content, now_millis,
                        )?;
                    }
                    Scene::Emote => {
                        self.append_emote_screen(
                            runtime, nodes, next, metrics, content, now_millis,
                        )?;
                    }
                    Scene::Loading => {
                        if let Some(stage) = self.loading_stage {
                            // An opaque cover under the loading screen: no partial terrain or
                            // HUD leaks through while the world settles.
                            nodes.push(
                                UiNode::new(UiNodeId::new(*next), None, viewport).with_visual(
                                    UiVisual::Solid {
                                        texture_page: self.solid_texture_page,
                                        color: [8, 10, 14, 255],
                                    },
                                ),
                            );
                            *next = next.saturating_add(1);
                            self.append_loading_screen(
                                runtime, stage, nodes, next, metrics, content,
                            )?;
                        }
                    }
                    Scene::SignEditor => {
                        self.append_sign_editor(
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content_width,
                            content_height,
                            now_millis,
                        )?;
                    }
                    Scene::ServerForm | Scene::ServerSettingsForm => {
                        self.append_server_form(
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content_width,
                            content_height,
                        )?;
                    }
                    Scene::Menu(_) => {
                        menu_hit_targets = self.append_menu(
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content_width,
                            content_height,
                        )?;
                    }
                    Scene::Credits => {
                        self.append_credits_screen(
                            runtime, nodes, next, metrics, content, now_millis,
                        )?;
                    }
                }
            }
            if !scenes.contains(&Scene::Chat) {
                self.close_chat_screen();
            }
            if !scenes.contains(&Scene::Emote) {
                self.close_emote_screen();
            }
            if !scenes.contains(&Scene::SignEditor) {
                self.hide_sign_editor();
            }
            // A client part's modal sits over gameplay only, below toasts and trusted chrome.
            let over_gameplay = self.menu_view.is_none()
                && self.loading_stage.is_none()
                && scenes
                    .iter()
                    .all(|scene| matches!(scene, Scene::Gameplay | Scene::Crosshair | Scene::Hud));
            self.append_experience_modal(
                runtime,
                &mut nodes,
                &mut next_id,
                metrics,
                content,
                over_gameplay,
            );
            // Toasts live on their own stack, drawn last over every scene.
            self.scene_clock.clear();
            self.append_toast_screen(
                runtime,
                &mut nodes,
                &mut next_id,
                metrics,
                content,
                now_millis,
            )?;
            self.sync_server_ui_pages();
            self.append_experience_chrome(
                runtime,
                &mut nodes,
                &mut next_id,
                metrics,
                [content_width, content_height],
            );
            self.append_mod_panel(runtime, &mut nodes, &mut next_id, metrics, content);
            let mut debug_eligibility =
                crate::ui_runtime::scene_stack::construction::DebugOverlayEligibility::default();
            for entry in stack.scenes() {
                debug_eligibility.push(entry.key, entry.settings);
            }
            if debug_eligibility.allowed() {
                self.append_debug_overlay(&mut nodes, &mut next_id, metrics, content)?;
            }
            // Every screen has painted: retire animation state nothing touched.
            self.append_oreui_motion(&mut nodes, &mut next_id, content)?;
            self.end_animation_frame();
            self.apply_gui_models(&mut nodes);
            let palette = self.formatting_palette().copied();
            let effects = TextEffects {
                palette: palette.as_ref(),
                obfuscation_seed: now_millis,
                obfuscation: Some(&self.obfuscation),
            };
            let render_viewport = UiRenderViewport {
                physical_size,
                dpi_scale,
                safe_area,
            };
            // `§k` text re-rolls its glyphs every frame, so it never reuses a drawn frame.
            let obfuscated = obfuscated(&nodes);
            let redrawn = if obfuscated {
                None
            } else {
                redraw_changed(
                    self.last_frame.as_mut(),
                    self.last_input.as_ref(),
                    &self.textures,
                    &nodes,
                    frame,
                    (viewport, render_viewport),
                    effects,
                )
                .inspect_err(|_| self.last_frame = None)?
            };
            if redrawn.is_none() {
                // A declined redraw leaves the retained frame stale.
                self.last_frame = None;
            }
            if let Some(redrawn) = redrawn {
                #[cfg(test)]
                {
                    self.redrawn_nodes += redrawn.emitted;
                }
                let input = match redrawn.input {
                    Redrawn::Unchanged(input) => input,
                    Redrawn::Vertices(input) => self.advance_revision(input),
                    Redrawn::Rebuilt(input) => self.stabilize_revision(input),
                };
                self.menu_hit_targets = menu_hit_targets;
                self.remember_menu(runtime, frame);
                return Ok(input);
            }
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("ui.geometry_rebuild").entered();
            #[cfg(test)]
            {
                self.tree_builds += 1;
            }
            #[cfg(feature = "tracy")]
            let _layout_span = bevy::log::info_span!("ui.layout_publish").entered();
            let draw =
                RetainedDraw::build(&nodes, viewport, UiScale::default(), safe_area, effects)
                    .map_err(UiPresentationError::Tree)?;
            let input = adapt_ui_draw_list(
                draw.draw_list(),
                Arc::clone(&self.textures),
                render_viewport,
            )
            .map_err(UiPresentationError::Adapter)?;
            self.last_frame = (!obfuscated).then(|| BuiltFrame {
                nodes: nodes.clone(),
                frame,
                textures: Arc::clone(&self.textures),
                draw,
            });
            let input = self.stabilize_revision(input);
            self.menu_hit_targets = menu_hit_targets;
            self.remember_menu(runtime, frame);
            Ok(input)
        })();
        self.assembly_nodes = nodes;
        result
    }
}

/// A frame's inputs: its nodes, viewport and texture array, and the draw list they made.
pub(super) struct BuiltFrame {
    pub(super) nodes: Vec<UiNode>,
    frame: ([u32; 2], f32, SafeArea),
    textures: Arc<UiRenderTextureArray>,
    /// The frame's draw list with each node's share of it.
    draw: RetainedDraw,
}

/// A frame drawn from the last one's draw list.
struct Redraw {
    input: Redrawn,
    /// Nodes emitted again.
    emitted: usize,
}

enum Redrawn {
    /// Every node matched: the last frame's input as published.
    Unchanged(UiRenderInput),
    /// Only vertices changed, and differ from the last frame's.
    Vertices(UiRenderInput),
    /// Indices or batches changed too.
    Rebuilt(UiRenderInput),
}

/// Draws `nodes` by emitting only nodes that differ from the last frame's, when the frame keeps
/// the last frame's tree shape, viewport and textures; `None` asks for a full build.
fn redraw_changed(
    last: Option<&mut BuiltFrame>,
    previous: Option<&UiRenderInput>,
    textures: &Arc<UiRenderTextureArray>,
    nodes: &[UiNode],
    frame: ([u32; 2], f32, SafeArea),
    (viewport, render_viewport): (UiRect, UiRenderViewport),
    effects: TextEffects<'_>,
) -> Result<Option<Redraw>, UiPresentationError> {
    let (Some(last), Some(previous)) = (last, previous) else {
        return Ok(None);
    };
    if last.frame != frame || !Arc::ptr_eq(&last.textures, textures) {
        return Ok(None);
    }
    let Some(update) = last.draw.update(
        &mut last.nodes,
        nodes,
        viewport,
        UiScale::default(),
        render_viewport.safe_area,
        effects,
    ) else {
        return Ok(None);
    };
    let input = if update.rebuilt {
        Redrawn::Rebuilt(
            adapt_ui_draw_list(last.draw.draw_list(), Arc::clone(textures), render_viewport)
                .map_err(UiPresentationError::Adapter)?,
        )
    } else {
        match render_adapter::patch_ui_vertices(
            previous,
            last.draw.draw_list(),
            &update.vertices,
            render_viewport.dpi_scale,
        )
        .map_err(UiPresentationError::Adapter)?
        {
            Some(input) => Redrawn::Vertices(input),
            None => Redrawn::Unchanged(previous.clone()),
        }
    };
    Ok(Some(Redraw {
        input,
        emitted: update.emitted,
    }))
}

/// Whether any text carries `§k`, whose glyphs change every frame.
fn obfuscated(nodes: &[UiNode]) -> bool {
    nodes.iter().any(|node| match node.visual() {
        UiVisual::Text { layout, .. } | UiVisual::RotatedText { layout, .. } => {
            layout.glyphs().iter().any(|glyph| glyph.style.obfuscated)
        }
        _ => false,
    })
}
