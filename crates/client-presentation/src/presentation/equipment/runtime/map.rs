//! Native paper bounds and hand stacks over the server's retained map pixels.

use super::*;
use bevy::math::{Mat4, Vec3};
use render::HandItemAtlas;
use render_model::ActorRigVertex;

const PAPER_BORDER: usize = 7;
const IMAGE_SIDE: usize = protocol::MAP_IMAGE_SIDE as usize;
const PAPER_SIDE: usize = IMAGE_SIDE + PAPER_BORDER * 2;

pub(super) struct MapRaster {
    key: (u64, Option<i64>, Option<u64>),
    background: Arc<[u8]>,
    atlas: HandItemAtlas,
}

impl EquipmentRuntime {
    /// A map uses its image identity, rather than the filled-map inventory icon.
    #[allow(clippy::too_many_arguments)]
    pub fn first_person_map(
        &mut self,
        body: &ActorRigSubmission,
        id: Option<i64>,
        image: Option<&client_world::MapImage>,
        hand: FirstPersonHand,
        pitch: f32,
        off_hand: bool,
        two_handed: bool,
    ) -> Option<(FirstPersonItem, HandItemAtlas)> {
        let (catalog, from_pack) = self
            .pack
            .as_ref()
            .filter(|pack| {
                pack.catalog.textures().iter().any(|texture| {
                    texture.identifier.as_ref() == assets::MAP_BACKGROUND_TEXTURE_IDENTIFIER
                })
            })
            .map(|pack| (Arc::clone(&pack.catalog), true))
            .or_else(|| Some((Arc::clone(self.catalog.as_ref()?), false)))?;
        let background = catalog.textures().iter().find(|texture| {
            texture.identifier.as_ref() == assets::MAP_BACKGROUND_TEXTURE_IDENTIFIER
        })?;
        let location = self
            .texture_location(&background.identifier, from_pack)?
            .single_layer_texture();
        let key = (
            body.input.identity.session_id,
            id,
            image.map(|image| image.revision),
        );
        let cache = &mut self.map_rasters[usize::from(off_hand)];
        if cache.as_ref().is_none_or(|cache| {
            cache.key != key || !Arc::ptr_eq(&cache.background, &background.rgba8)
        }) {
            *cache = Some(MapRaster {
                key,
                background: Arc::clone(&background.rgba8),
                atlas: paper_atlas(background, image)?,
            });
        }
        let atlas = cache.as_ref()?.atlas.clone();
        let mesh = match self.meshes.get(&MeshKey::Map).copied().flatten() {
            Some(mesh) => mesh,
            None => {
                let mesh = self.build_item_mesh(|id| {
                    ActorRigGeometry::new(id, paper_vertices(), vec![[0.0; 3]]).ok()
                })?;
                self.meshes.insert(MeshKey::Map, Some(mesh));
                mesh
            }
        };
        let matrix = map_pose(hand, pitch, off_hand, two_handed)?;
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        let bone = view_bone(ItemDisplay {
            rotation,
            translation,
            scale: scale.x,
        })?;
        let layer = if off_hand {
            LAYER_OFF_HAND
        } else {
            LAYER_MAIN_HAND
        };
        let poses = self.poses.share(
            body,
            if off_hand {
                FIRST_PERSON_OFFHAND_LAYER
            } else {
                FIRST_PERSON_ITEM_LAYER
            },
            [&[bone], &[bone]],
        );
        Some((
            FirstPersonItem {
                presentation: layer_presentation(body, layer, mesh, poses, location, 0),
                camera_space: true,
                alpha_mode: render::HandItemAlphaMode::Cutout,
                java_camera: None,
                java_normal_axis: Vec3::Z,
            },
            atlas,
        ))
    }
}

/// Composes retained map pixels over the selected pack's paper background.
fn paper_atlas(
    background: &assets::EquipmentTexture,
    image: Option<&client_world::MapImage>,
) -> Option<HandItemAtlas> {
    let (width, height) = (
        usize::from(background.width),
        usize::from(background.height),
    );
    if width == 0 || height == 0 || background.rgba8.len() != width * height * 4 {
        return None;
    }
    let mut pixels = vec![0; PAPER_SIDE * PAPER_SIDE * 4];
    for y in 0..PAPER_SIDE {
        for x in 0..PAPER_SIDE {
            let source = ((y * height / PAPER_SIDE) * width + x * width / PAPER_SIDE) * 4;
            let target = (y * PAPER_SIDE + x) * 4;
            pixels[target..target + 4].copy_from_slice(&background.rgba8[source..source + 4]);
        }
    }
    if let Some(image) = image.filter(|image| image.pixels.len() == IMAGE_SIDE * IMAGE_SIDE) {
        for (index, pixel) in image.pixels.iter().enumerate() {
            let [red, green, blue, alpha] = pixel.to_le_bytes();
            let offset = ((index / IMAGE_SIDE + PAPER_BORDER) * PAPER_SIDE
                + index % IMAGE_SIDE
                + PAPER_BORDER)
                * 4;
            let source_alpha = u32::from(alpha);
            let background_alpha = u32::from(pixels[offset + 3]);
            let combined_alpha = source_alpha * 255 + background_alpha * (255 - source_alpha);
            for (channel, value) in [red, green, blue].into_iter().enumerate() {
                if combined_alpha != 0 {
                    pixels[offset + channel] = ((u32::from(value) * source_alpha * 255
                        + u32::from(pixels[offset + channel])
                            * background_alpha
                            * (255 - source_alpha))
                        / combined_alpha) as u8;
                }
            }
            pixels[offset + 3] = (combined_alpha / 255) as u8;
        }
    }
    Some(HandItemAtlas {
        width: PAPER_SIDE as u16,
        height: PAPER_SIDE as u16,
        layers: 1,
        rgba8: pixels.into(),
    })
}

/// Builds the shared map plane, including the paper margin around the image.
fn paper_vertices() -> Vec<ActorRigVertex> {
    let low = -(PAPER_BORDER as f32);
    let high = (IMAGE_SIDE + PAPER_BORDER) as f32;
    let corners = [
        [low, high, 0.0],
        [high, high, 0.0],
        [high, low, 0.0],
        [low, low, 0.0],
    ];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    [0, 1, 2, 0, 2, 3]
        .map(|index| ActorRigVertex {
            position: corners[index],
            normal: [0.0, 0.0, -1.0],
            uv: uvs[index],
            back_uv: uvs[index],
            ..Default::default()
        })
        .into()
}

/// Places the map in view space using independent equip state and sampled attack/pitch.
fn map_pose(hand: FirstPersonHand, pitch: f32, off_hand: bool, two_handed: bool) -> Option<Mat4> {
    if !hand.swing.is_finite() || !hand.equip.is_finite() || !pitch.is_finite() {
        return None;
    }
    let attack = if off_hand {
        0.0
    } else {
        hand.swing.clamp(0.0, 1.0)
    };
    let root_swing = (attack.sqrt() * std::f32::consts::PI).sin();
    let swing = Vec3::new(
        if two_handed { -0.4 * root_swing } else { 0.0 },
        0.2 * (2.0 * attack.sqrt() * std::f32::consts::PI).sin(),
        -0.2 * (attack * std::f32::consts::PI).sin(),
    );
    let tilt = if two_handed {
        (-((1.1 - pitch / 45.0).clamp(0.0, 1.0) * std::f32::consts::PI).cos() + 1.0) * 0.5
    } else {
        0.0
    };
    let base = Vec3::new(
        0.0,
        0.04 - 1.2 * (1.0 - hand.equip.clamp(0.0, 1.0)) - 0.5 * tilt,
        -0.72,
    );
    let rotation = Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2)
        * Mat4::from_rotation_z((-85.0 * tilt).to_radians())
        * Mat4::from_rotation_y(
            (-20.0 * (attack * attack * std::f32::consts::PI).sin()).to_radians(),
        )
        * Mat4::from_rotation_z((-20.0 * root_swing).to_radians())
        * Mat4::from_rotation_y((-80.0 * root_swing).to_radians());
    let (offset, divisor) = if two_handed {
        (-1.0, IMAGE_SIDE as f32 / 2.0)
    } else {
        (if off_hand { -1.85 } else { 0.85 }, IMAGE_SIDE as f32)
    };
    Some(
        Mat4::from_translation(swing + base)
            * rotation
            * Mat4::from_scale(Vec3::splat(0.38))
            * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2)
            * Mat4::from_rotation_z(std::f32::consts::PI)
            * Mat4::from_translation(Vec3::new(offset, -1.0, 0.0))
            * Mat4::from_scale(Vec3::splat(divisor.recip())),
    )
}

#[cfg(test)]
mod tests;
