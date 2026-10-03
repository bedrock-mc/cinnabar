//! Player capes: the cape geometry drawn with the player's own pose and a cape raster carried
//! in the skin layer payload.
use std::sync::Arc;

use assets::RuntimeEntityAssets;
use client_world::{ActorRigSnapshot, PlayerProfile};
use protocol::PlayerSkin;
use render::{ACTOR_LAYER_BODY, ActorRigRoute, ActorRigSubmission, MAX_RENDERED_PLAYERS};

use super::actors::ActorPresentationBatch;

pub(crate) use render::cape::{ACTOR_LAYER_CAPE, CapeRig, cape_layer, cape_pose};
/// The cape rig, resolved on first use; an absent geometry leaves capes undrawn.
#[derive(Default)]
pub(crate) struct CapeState {
    resolved: bool,
    rig: Option<CapeRig>,
}

impl CapeState {
    pub(crate) fn rig(&mut self, assets: Option<&RuntimeEntityAssets>) -> Option<&CapeRig> {
        if !self.resolved
            && let Some(assets) = assets
        {
            self.resolved = true;
            self.rig = CapeRig::resolve(assets);
        }
        self.rig.as_ref()
    }
}

fn cape_of(profile: &PlayerProfile) -> Option<Arc<[u8]>> {
    match &profile.skin {
        PlayerSkin::Standard(skin) => {
            let cape = skin.cape.as_ref()?;
            cape_layer(cape.width, cape.height, &cape.rgba8)
        }
        PlayerSkin::Unavailable(_) => None,
    }
}

/// Appends a cape instance for every drawn player body whose skin carries one; capes past the
/// skin layer budget are dropped rather than invalidating the frame.
pub(crate) fn apply_capes<'a>(
    batch: &mut ActorPresentationBatch,
    cape: &CapeRig,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    profile_of: impl Fn(u64) -> Option<&'a PlayerProfile>,
) {
    let mut capes: Vec<(Arc<[u8]>, usize)> = Vec::new();
    let mut extras = Vec::new();
    for body in &batch.submissions {
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY
            || body.route == ActorRigRoute::NoDraw
            || batch.artwork.contains_key(&identity)
        {
            continue;
        }
        let Some(cape_pixels) = profile_of(identity.runtime_id).and_then(cape_of) else {
            continue;
        };
        let Some(rig) = rig_of(identity.runtime_id) else {
            continue;
        };
        let layer = match capes.iter().find(|(known, _)| *known == cape_pixels) {
            Some((_, layer)) => *layer,
            None if batch.skin_layers.len() < MAX_RENDERED_PLAYERS => {
                batch.skin_layers.push(Arc::clone(&cape_pixels));
                capes.push((cape_pixels, batch.skin_layers.len() - 1));
                batch.skin_layers.len() - 1
            }
            None => continue,
        };
        let mut submission: ActorRigSubmission = body.clone();
        submission.input.identity.layer = ACTOR_LAYER_CAPE;
        submission.input.rig = cape.id;
        submission.input.previous_bones =
            cape_pose(cape, rig.bone_names, &body.input.previous_bones);
        submission.input.current_bones = cape_pose(cape, rig.bone_names, &body.input.current_bones);
        submission.texture_layer = layer as u32;
        submission.tint = 0;
        submission.overlay_rgba8 = 0;
        extras.push(submission);
    }
    batch.submissions.extend(extras);
}

#[cfg(test)]
mod tests {
    use super::cape_layer;
    use render::cape::multiply;

    #[test]
    fn cape_rasters_resample_onto_one_skin_layer() {
        let mut cape = vec![0u8; 64 * 32 * 4];
        cape[..4].copy_from_slice(&[9, 8, 7, 255]);
        let layer = cape_layer(64, 32, &cape).unwrap();
        assert_eq!(layer.len(), render::STANDARD_SKIN_BYTES);
        assert_eq!(&layer[..4], &[9, 8, 7, 255]);
        let row = render::STANDARD_SKIN_SIDE * 4;
        let rows_per_source = render::STANDARD_SKIN_SIDE / 32;
        assert_eq!(&layer[row..row + 4], &[9, 8, 7, 255]);
        let next = rows_per_source * row;
        assert_eq!(&layer[next..next + 4], &[0, 0, 0, 0]);
        assert!(cape_layer(64, 32, &cape[1..]).is_none());
    }

    #[test]
    fn quaternion_product_composes_the_half_turn_with_a_tilt() {
        let turn = [0.0, 1.0, 0.0, 0.0];
        assert_eq!(multiply(turn, [0.0, 0.0, 0.0, 1.0]), turn);
        let twice = multiply(turn, turn);
        assert_eq!(twice, [0.0, 0.0, 0.0, -1.0]);
    }
}
