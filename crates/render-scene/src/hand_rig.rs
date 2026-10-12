use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_api::SkinRgba8;
use render_model::{ActorRigRenderFrame, EquipmentRaster, HandItemAtlas, HandRigLight};
/// One frame's local first-person arms and held items in camera space, with their
/// artwork, lighting, and base FOV.
#[derive(Clone, Debug)]
pub struct HandRigFrame {
    pub rig: ActorRigRenderFrame,
    pub skin: SkinRgba8,
    pub light: HandRigLight,
    pub fov_radians: f32,
    pub revision: u64,
    pub item_atlases: [Option<HandItemAtlas>; 2],
    /// The held-item glint raster for item instances drawn with the glint material.
    pub item_glint: Option<EquipmentRaster>,
}

/// Published by the app each frame the first-person hand should draw; empty otherwise.
#[derive(Clone, Default, Debug, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct HandRigScene {
    pub frame: Option<HandRigFrame>,
}

impl HandRigScene {
    pub fn clear(&mut self) {
        self.frame = None;
    }

    /// Shares the skin and projection admission used by early first-person readiness.
    pub fn accepts_skin_and_fov(skin: &SkinRgba8, fov_radians: f32) -> bool {
        render_model::actor_skin_side(skin).is_some()
            && fov_radians > 0.0
            && fov_radians < std::f32::consts::PI
    }

    /// Accepts a single-instance rig frame with an admitted square skin and a finite positive FOV;
    /// anything else clears the scene so the fallback keeps rendering.
    pub fn publish(
        &mut self,
        rig: ActorRigRenderFrame,
        skin: SkinRgba8,
        light: HandRigLight,
        fov_radians: f32,
        revision: u64,
    ) -> bool {
        if rig.instances.is_empty()
            || rig.previous_bones.is_empty()
            || rig.previous_bones.len() != rig.current_bones.len()
            || rig.maximum_vertex_count == 0
            || !Self::accepts_skin_and_fov(&skin, fov_radians)
            || revision == 0
        {
            self.clear();
            return false;
        }
        self.frame = Some(HandRigFrame {
            rig,
            skin,
            light,
            fov_radians,
            revision,
            item_atlases: [None, None],
            item_glint: None,
        });
        true
    }

    /// Supplies the atlas page for item instances of the published frame; ignored when inactive
    /// or when the page is malformed.
    pub fn set_item_atlases(&mut self, atlases: [Option<HandItemAtlas>; 2]) {
        let valid = |atlas: &HandItemAtlas| {
            atlas.width != 0
                && atlas.height != 0
                && atlas.layers != 0
                && usize::from(atlas.width)
                    .checked_mul(usize::from(atlas.height))
                    .and_then(|pixels| pixels.checked_mul(atlas.layers as usize))
                    .and_then(|pixels| pixels.checked_mul(4))
                    == Some(atlas.rgba8.len())
        };
        if let Some(frame) = &mut self.frame {
            frame.item_atlases = atlases.map(|atlas| atlas.filter(valid));
        }
    }

    /// Supplies the held-item glint raster of the published frame; ignored when inactive or
    /// when the raster is malformed.
    pub fn set_item_glint(&mut self, glint: Option<EquipmentRaster>) {
        let valid = |raster: &EquipmentRaster| {
            raster.width != 0
                && raster.height != 0
                && raster.rgba8.len() == usize::from(raster.width) * usize::from(raster.height) * 4
        };
        if let Some(frame) = &mut self.frame {
            frame.item_glint = glint.filter(valid);
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.frame.is_some()
    }
}
