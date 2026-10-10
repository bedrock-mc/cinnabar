//! Presentation authority shared with app adapters.

use super::*;

impl UiPresentationRuntime {
    /// Counts and rate-limits rejected UI frames without changing session authority.
    pub fn record_frame_failure(&mut self, error: &UiPresentationError) {
        self.rejected_frames = self.rejected_frames.saturating_add(1);
        if self.rejected_frames.is_power_of_two() {
            bevy::log::warn!(
                rejected_frames = self.rejected_frames,
                ?error,
                "skipped invalid UI frame"
            );
        }
    }

    /// Returns the number of rejected UI frames in this presentation runtime.
    pub const fn rejected_frame_count(&self) -> u64 {
        self.rejected_frames
    }

    pub fn set_loading_stage(&mut self, stage: Option<LoadingStage>) {
        self.loading_stage = stage;
    }

    pub fn set_menu_view(&mut self, view: Option<MenuView>) {
        if let Some(requests) = self.base_font.glyph_requests()
            && let Some(view) = &view
        {
            requests.set_locale(view.settings_options.language().unwrap_or(""));
        }
        self.poll_font_fallback();
        if let (Some(current), Some(incoming)) = (&self.menu_view, &view)
            && current.as_ref() == incoming
        {
            return;
        }
        self.menu_view = view.map(Arc::new);
    }

    pub fn hit_test_menu(&self, position: UiPoint) -> Option<MenuAction> {
        self.menu_hit_targets
            .iter()
            .rev()
            .find_map(|(action, bounds)| bounds.contains(position).then_some(*action))
    }

    /// Selects a fixed desktop GUI scale; `None` or 0 restores auto.
    pub fn set_gui_scale_preference(&mut self, preference: Option<u8>) {
        self.gui_scale_preference = preference.filter(|value| *value > 0);
    }

    /// Applies the platform safe area to layout and render clipping.
    /// A viewport too small for the fixed HUD shows no HUD.
    pub fn set_safe_area(&mut self, safe_area: SafeArea) {
        self.safe_area = safe_area;
    }

    /// Borrows the captured HUD values for the app's publication adapters.
    pub fn hud_frame(&self) -> &HudFrame {
        &self.hud_frame
    }

    pub fn hud_frame_mut(&mut self) -> &mut HudFrame {
        &mut self.hud_frame
    }

    /// Supplies the world-query adapter's projected nametag anchors for this frame.
    pub fn set_nametag_anchors(&mut self, anchors: Vec<nametags::NametagAnchor>) {
        self.nametag_anchors = anchors;
    }

    /// The world-space tag quads for this frame's anchors.
    pub fn nametag_scene(&mut self) -> render_model::NametagScene {
        let palette = self.formatting_palette().copied().unwrap_or_default();
        self.nametag_atlas.set_palette(palette);
        let (font, glyphs) = (&self.font, &self.session_glyphs);
        let dynamic_start = self.textures.dynamic_start();
        nametags::build_nametag_scene(
            &self.nametag_anchors,
            font,
            &mut self.layouts,
            &mut self.nametag_atlas,
            &|page| {
                nametag_atlas::font_page(font, page).or_else(|| glyphs.page(dynamic_start, page))
            },
        )
    }

    /// Retained text-layout cache entries, exposed for the bounded-memory
    /// steady-state witnesses.
    #[cfg(test)]
    pub fn layout_cache_len(&self) -> usize {
        self.layouts.len()
    }

    /// Borrows loading readiness for the app's existing ordered preparation stage.
    pub fn startup_mut(&mut self) -> &mut startup::StartupPresentationState {
        &mut self.startup
    }
    /// Borrows loading readiness without advancing it.
    pub fn startup(&self) -> &startup::StartupPresentationState {
        &self.startup
    }
    /// Reports the currently presented loading stage to the app's hand adapter.
    pub fn loading_stage(&self) -> Option<LoadingStage> {
        self.loading_stage
    }
    /// Advances paper-doll presentation from the app's synchronous movement observation.
    pub fn observe_paper_doll(&mut self, now: u64, state: Option<paper_doll::State>) {
        self.hud_frame.paper_doll_visible = self.paper_doll.update(now, state);
    }

    /// The effective GUI preference consumed by the app scale adapter.
    pub fn gui_scale_preference(&self) -> Option<u8> {
        self.gui_scale_preference
    }
}
