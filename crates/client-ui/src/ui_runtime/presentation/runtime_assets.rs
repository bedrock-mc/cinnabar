//! Skin and menu artwork updates for retained presentation.

use super::*;

impl UiPresentationRuntime {
    /// Retains original UI skin pixels and compatibility hand rasters. Live model view/bob
    /// changes update geometry without regenerating a thumbnail or reuploading texture pixels.
    pub fn set_player_preview_skin(
        &mut self,
        skin: Option<&[u8]>,
        pose: player_preview::PlayerPreviewPose,
    ) {
        self.set_player_preview_poses(skin, pose, pose);
    }

    /// Poses the model at `pose` and draws the software rasters at `raster`. With model
    /// geometry the preview raster is blank and the hand rasters read only pitch and sneaking,
    /// so a turn changes geometry alone and never rebuilds a texture page.
    pub(super) fn set_player_preview_poses(
        &mut self,
        skin: Option<&[u8]>,
        pose: player_preview::PlayerPreviewPose,
        raster: player_preview::PlayerPreviewPose,
    ) {
        let default_skin = render_model::default_actor_skin_rgba8();
        let skin = skin
            .filter(|pixels| {
                let side = (pixels.len() / 4).isqrt();
                side != 0 && side * side * 4 == pixels.len()
            })
            .unwrap_or(default_skin.as_ref());
        let source_hash = self
            .set_gui_skin(skin)
            .unwrap_or_else(|| Sha256::digest(skin).into());
        let drawn = if self.gui_models.enabled {
            // Model pose and sway now only change geometry. Keep the software hand carriers
            // cached by skin/hand pose; they must not force a small model render/upload each frame.
            (
                player_preview::PreviewView::default(),
                0.0,
                Default::default(),
            )
        } else {
            (
                self.player_preview_view,
                self.player_preview_bob,
                self.player_preview_gear.clone(),
            )
        };
        let raster = if self.gui_models.enabled {
            player_preview::PlayerPreviewPose::new(0.0, 0.0, raster.pitch_degrees, raster.sneaking)
        } else {
            raster
        };
        self.player_preview_pose = Some(pose);
        if self.player_preview_source_hash == Some(source_hash)
            && self.player_preview_raster_pose == Some(raster)
            && self.player_preview_drawn.as_ref() == Some(&drawn)
        {
            return;
        }
        self.player_preview_pixels = Some(player_preview::PlayerPreviewRasters {
            preview: if self.gui_models.enabled {
                // The retained reference supplies the JSON-UI model's virtual coordinate basis.
                // No thumbnail is drawn: apply_gui_models replaces it with destination geometry.
                vec![
                    0;
                    (player_preview::PREVIEW_WIDTH * player_preview::PREVIEW_HEIGHT * 4) as usize
                ]
            } else {
                match self.menu_preview_model.vertices.as_deref() {
                    Some(body) => player_preview::render_body_with_cape(
                        body,
                        skin,
                        raster,
                        drawn.0,
                        drawn.1,
                        &drawn.2,
                        self.menu_preview_model.cape.as_ref(),
                    ),
                    None => player_preview::render(skin, raster, drawn.0, drawn.1, &drawn.2),
                }
            },
            left_hand: player_preview::render_hand(skin, raster, true),
            right_hand: player_preview::render_hand(skin, raster, false),
        });
        self.player_preview_drawn = Some(drawn);
        self.player_preview_source_hash = Some(source_hash);
        self.player_preview_raster_pose = Some(raster);
        self.preview_dirty = true;
        self.rebuild_dynamic_textures();
    }

    pub const fn player_preview_icon(&self) -> Option<IconRef> {
        self.player_preview_icon
    }

    pub const fn player_hand_icons(&self) -> (Option<IconRef>, Option<IconRef>) {
        (self.left_hand_icon, self.right_hand_icon)
    }

    /// Rebuilds pages after their retained artwork or raster sources change.
    pub(in super::super) fn rebuild_dynamic_textures(&mut self) {
        dynamic_textures::rebuild(self);
    }

    /// Service art at `paths`, plus the engine's oversized textures, on the art
    /// pages once the worker has packed them; the last atlas draws meanwhile.
    pub fn sync_menu_artwork(&mut self, paths: Vec<(String, u32)>) {
        let set = menu_artwork::ArtworkSet {
            paths,
            oversized: self.oversized_ui_textures(),
            ..Default::default()
        };
        self.sync_artwork_set(set);
    }

    /// Requests a changed artwork set and installs completed pages.
    pub(super) fn sync_artwork_set(&mut self, set: menu_artwork::ArtworkSet) {
        if !set.same(&self.menu_artwork_set) {
            self.menu_artwork_set = set.clone();
            self.menu_artwork_loader.request(set);
        }
        if self.menu_artwork_loader.poll() {
            self.rebuild_dynamic_textures();
        }
    }

    /// Installs the latest requested art set's complete atlas.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_menu_artwork(&mut self) {
        self.menu_artwork_loader.wait();
        self.rebuild_dynamic_textures();
    }

    pub fn menu_artwork_icon(&self, path: &str) -> Option<IconRef> {
        self.menu_artwork.refs.get(path).copied()
    }
}
