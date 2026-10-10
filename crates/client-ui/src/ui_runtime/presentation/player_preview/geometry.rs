//! Direct JSON-UI model geometry: skin texels are sampled at the destination
//! framebuffer's physical resolution, never from the small CPU preview raster.

use std::sync::Arc;

use render_model::{ActorVertex, standard_biped_overlay_vertices, standard_biped_vertices};
use ui::{UI_STYLE_GLINT, UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

use super::{
    IconRef, PREVIEW_HEIGHT, PREVIEW_WIDTH, PlayerPreviewPose, PreviewEquipment, PreviewHeldModel,
    PreviewView, Rig, equipment,
};

mod fire;
mod held;
mod lighting;
pub use fire::PreviewFire;

/// The same native live-player/paper-doll projection as the retained frame,
/// with the original skin and equipment texture regions. The frame's virtual
/// coordinates are only a matrix basis; they impose no raster-size ceiling.
///
/// Player's default `entity_alphatest` material has point sampling and a 0.5
/// alpha cutoff. `fancy` chooses the material-list's native directional shader
/// variant; brightness remains a float varying, separate from sRGB dye color.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::too_many_arguments)]
pub fn mesh(
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    skin: IconRef,
    gear: &PreviewEquipment,
    armor: [Option<IconRef>; 4],
    hands: [Option<&PreviewHeldModel>; 2],
    fancy: bool,
) -> Option<Arc<UiMesh>> {
    mesh_with_body(
        None, pose, view, bob, skin, gear, armor, hands, fancy, None, [0.0; 4],
    )
}

/// Uses already posed world geometry for the HUD, retaining the shared UI shading and depth path.
#[allow(clippy::too_many_arguments)]
pub fn mesh_with_body(
    body: Option<(&[ActorVertex], &[Option<bevy::math::Affine3A>; 6])>,
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    skin: IconRef,
    gear: &PreviewEquipment,
    armor: [Option<IconRef>; 4],
    hands: [Option<&PreviewHeldModel>; 2],
    fancy: bool,
    fire: Option<PreviewFire>,
    overlay_color: [f32; 4],
) -> Option<Arc<UiMesh>> {
    mesh_with_cape(
        body,
        pose,
        view,
        bob,
        skin,
        gear,
        armor,
        hands,
        fancy,
        fire,
        overlay_color,
        None,
    )
}

/// Samples an attached cape from its original texture alongside the skin and equipment.
#[allow(clippy::too_many_arguments)]
pub fn mesh_with_cape(
    body: Option<(&[ActorVertex], &[Option<bevy::math::Affine3A>; 6])>,
    pose: PlayerPreviewPose,
    view: PreviewView,
    bob: f32,
    skin: IconRef,
    gear: &PreviewEquipment,
    armor: [Option<IconRef>; 4],
    hands: [Option<&PreviewHeldModel>; 2],
    fancy: bool,
    fire: Option<PreviewFire>,
    overlay_color: [f32; 4],
    cape: Option<(&[ActorVertex], IconRef)>,
) -> Option<Arc<UiMesh>> {
    let mut rig = Rig::new(pose, view, bob, hands.map(|model| model.is_some()));
    if let Some((_, parts)) = body {
        rig.parts = *parts;
    }
    let mut vertices = Vec::new();
    let mut batches = Vec::new();
    let mut skin_vertices = standard_biped_vertices();
    skin_vertices.extend(standard_biped_overlay_vertices());
    append(
        &mut vertices,
        &mut batches,
        &rig,
        body.map_or(skin_vertices.as_slice(), |(vertices, _)| vertices),
        skin,
        None,
        fancy,
    )?;
    if let Some((cape, icon)) = cape {
        append(&mut vertices, &mut batches, &rig, cape, icon, None, fancy)?;
    }
    for (slot, icon) in armor.into_iter().enumerate() {
        let (Some(icon), Some(texture)) = (icon, &gear.armor[slot]) else {
            continue;
        };
        append(
            &mut vertices,
            &mut batches,
            &rig,
            &equipment::armor_vertices(slot),
            icon,
            texture.tint,
            fancy,
        )?;
    }
    for vertex in &mut vertices {
        vertex.overlay_color = overlay_color;
    }
    for (hand, model) in hands.into_iter().enumerate() {
        if let Some(model) = model {
            held::append(&mut vertices, &mut batches, &rig, model, hand, fancy)?;
        }
    }
    if let Some(fire) = fire {
        fire::append(&mut vertices, &mut batches, fire)?;
    }
    normalize_depth(&mut vertices)?;
    let indices: Vec<_> = (0..u32::try_from(vertices.len()).ok()?).collect();
    UiMesh::new(vertices.into(), indices.into(), batches.into())
        .ok()
        .map(Arc::new)
}

fn append(
    vertices: &mut Vec<UiMeshVertex>,
    batches: &mut Vec<UiMeshBatch>,
    rig: &Rig,
    source: &[ActorVertex],
    icon: IconRef,
    tint: Option<[u8; 3]>,
    fancy: bool,
) -> Option<()> {
    let [left, top, right, bottom] = icon.uv;
    if left >= right || top >= bottom || !source.len().is_multiple_of(3) {
        return None;
    }
    let start = u32::try_from(vertices.len()).ok()?;
    let [red, green, blue] = tint.unwrap_or([255; 3]);
    let centers = lighting::part_centers(source);
    for triangle in source.as_chunks::<3>().0 {
        let projected = [0, 1, 2].map(|corner| rig.project(triangle[corner]));
        let model_light = lighting::triangle_light(
            triangle,
            projected.map(|vertex| vertex.world),
            *centers.get(&triangle[0].part)?,
            fancy,
        )?;
        for (vertex, projected) in triangle.iter().zip(projected) {
            if projected
                .screen
                .iter()
                .chain(projected.world.iter())
                .chain(vertex.uv.iter())
                .any(|value| !value.is_finite())
            {
                return None;
            }
            let uv = [
                atlas_edge(left, right, vertex.uv[0])?,
                atlas_edge(top, bottom, vertex.uv[1])?,
            ];
            vertices.push(UiMeshVertex {
                position: [
                    projected.screen[0] / PREVIEW_WIDTH as f32,
                    projected.screen[1] / PREVIEW_HEIGHT as f32,
                ],
                clip_z: projected.world[2],
                clip_w: 1.0,
                uv,
                color: [red, green, blue, 255],
                model_light,
                overlay_color: [0.0; 4],
                style_flags: if icon.glint { UI_STYLE_GLINT } else { 0 }
                    | if tint.is_some() {
                        render_model::UI_STYLE_COLOR_MASK as u8
                    } else {
                        0
                    },
                alpha_test: false,
            });
        }
    }
    batches.push(UiMeshBatch {
        texture_page: icon.page,
        index_range: start..u32::try_from(vertices.len()).ok()?,
        blend: UiBlendMode::Alpha,
        depth_test: true,
        depth_write: true,
        alpha_cutoff: Some(if tint.is_some() { 1.0 / 255.0 } else { 0.5 }),
    });
    Some(())
}

/// Original edges and side-face texel centers remain exact floating values.
/// Reject malformed mappings, never round them into an adjacent texel.
fn atlas_edge(low: u16, high: u16, fraction: f32) -> Option<f32> {
    let edge = f32::from(low) + fraction * f32::from(high.checked_sub(low)?);
    (edge.is_finite() && edge >= f32::from(low) && edge <= f32::from(high)).then_some(edge)
}

/// Map all model parts into one isolated reverse-Z interval. Reserving the
/// outer quarters keeps every fragment strictly inside the UI depth clear and
/// near plane, including the farthest face; it changes no projected position.
fn normalize_depth(vertices: &mut [UiMeshVertex]) -> Option<()> {
    let mut far = f32::INFINITY;
    let mut near = f32::NEG_INFINITY;
    for vertex in vertices.iter() {
        far = far.min(vertex.clip_z);
        near = near.max(vertex.clip_z);
    }
    let span = near - far;
    if !span.is_finite() || span <= 0.0 {
        return None;
    }
    for vertex in vertices {
        vertex.clip_z = 0.25 + (vertex.clip_z - far) / span * 0.5;
    }
    Some(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hd_armor_tests;

#[cfg(any(test, feature = "test-support"))]
pub use held::test_support::assert_installed_shield;
