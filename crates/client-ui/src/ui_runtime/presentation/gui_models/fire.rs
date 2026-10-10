//! Original actor flame atlas frames occupy the existing bounded model-source pages.

use assets::ParticleTexture;

use super::{MODEL_PAGE, MODEL_PAGES, UiPresentationError, UiPresentationRuntime, atlas};
use crate::ui_runtime::presentation::{IconRef, player_preview};

#[derive(Default)]
pub(super) struct FireAtlas {
    source: Option<ParticleTexture>,
    pub(super) frames: Vec<IconRef>,
    pub(super) pages_start: Option<usize>,
}

#[derive(Default)]
pub(super) struct FirePlayback {
    actor: Option<(u64, u64, u64)>,
    frame: usize,
    countdown: f32,
}

impl FirePlayback {
    pub(super) fn observe(&mut self, actor: (u64, u64, u64), burning: bool, frames: usize) {
        if self.actor != Some(actor) {
            *self = Self {
                actor: Some(actor),
                ..Default::default()
            };
        }
        if !burning || frames == 0 {
            return;
        }
        // The HUD advances the flame countdown by one per visible draw.
        self.countdown -= 1.0;
        // Advance at most once per draw, reset to two and discard overshoot.
        if self.countdown <= 0.0 {
            self.countdown = 2.0;
            self.frame = (self.frame + 1) % frames;
        }
    }
}

impl UiPresentationRuntime {
    /// Optional particle carriers supply native flames without synthesizing replacement artwork.
    pub fn set_gui_fire_texture(
        &mut self,
        texture: Option<&ParticleTexture>,
    ) -> Result<(), UiPresentationError> {
        if let Some(start) = self.gui_models.fire.pages_start {
            self.gui_models.pages.truncate(start);
        }
        self.gui_models.fire.source = texture.filter(|texture| valid(texture)).cloned();
        self.install_gui_fire()?;
        self.place_pack_armor();
        Ok(())
    }

    pub(super) fn install_gui_fire(&mut self) -> Result<(), UiPresentationError> {
        self.gui_models.fire.frames.clear();
        let mut atlas = atlas::Atlas::new(0, MODEL_PAGES);
        let mut frames = Vec::new();
        if let Some(texture) = &self.gui_models.fire.source {
            let side = texture.width as u16;
            let frame_bytes = usize::from(side) * usize::from(side) * 4;
            for pixels in texture.rgba8.chunks_exact(frame_bytes) {
                frames.push(atlas.insert([side; 2], pixels)?);
            }
        }
        let (pages, _) = atlas.finish()?;
        let mut start = self.gui_models.pages.len();
        if start + pages.len() > MODEL_PAGES
            && self
                .gui_models
                .discard_optional_models(self.textures.dynamic_start() + MODEL_PAGE)
        {
            start = self.gui_models.pages.len();
        }
        if start + pages.len() > MODEL_PAGES {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let first = (self.textures.dynamic_start() + MODEL_PAGE + start) as u16;
        for frame in &mut frames {
            frame.page += first;
        }
        self.gui_models.fire.frames = frames;
        self.gui_models.fire.pages_start = Some(start);
        self.gui_models.pages.extend(pages);
        Ok(())
    }

    pub(super) fn gui_fire_frame(&self) -> Option<player_preview::geometry::PreviewFire> {
        if self.player_preview_view != player_preview::PreviewView::Hud {
            return None;
        }
        let size = self.gui_models.live_player.fire_size?;
        let frames = &self.gui_models.fire.frames;
        if frames.is_empty() {
            return None;
        }
        let frame = self.gui_models.live_player.fire.frame % frames.len();
        Some(player_preview::geometry::PreviewFire {
            texture: frames[frame],
            size,
            outer_y: self.gui_models.live_player.outer_y,
        })
    }
}

fn valid(texture: &ParticleTexture) -> bool {
    texture.path.as_ref() == assets::ACTOR_FLAME_TEXTURE
        && texture.width > 0
        && texture.width <= render_model::UI_MODEL_ATLAS_SIDE - 2
        && texture.height > 0
        && texture.height.is_multiple_of(texture.width)
        && texture
            .width
            .checked_mul(texture.height)
            .and_then(|pixels| pixels.checked_mul(4))
            == u32::try_from(texture.rgba8.len()).ok()
}

/// Vanilla's inherited player overlay defaults; damage takes precedence over fire.
pub(super) fn native_player_overlay(actor: &client_world::ActorSnapshot) -> [f32; 4] {
    if actor.status.overlay_active() {
        return [1.0, 0.0, 0.0, 0.25];
    }
    let Some(ticks) = actor.status.on_fire_time() else {
        return [0.0; 4];
    };
    let fade = if actor.is_on_fire() {
        ticks as f32
    } else {
        client_world::FIRE_FADE_TICKS.saturating_sub(ticks) as f32
    } / client_world::FIRE_FADE_TICKS as f32;
    let blend = fade.clamp(0.0, 1.0);
    let alpha = blend * blend * (3.0 - 2.0 * blend) * 0.7;
    // This is the authored Molang pulse period in ticks, independent of the simulation rate.
    let pulse = ((ticks as f32 / 20.0) * std::f32::consts::TAU).sin();
    [0.8, 0.3 + (0.15 - 0.3) * pulse * pulse, 0.0, alpha]
}

#[cfg(test)]
mod tests;
