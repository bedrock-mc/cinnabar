//! Native materials whose texture or depth composition is not represented by a pack draw.

use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use render_model::{ActorGpuInstance, BlockEntityFrame};
use render_scene::{ActorRenderFrame, HandRigScene};

/// Keeps native mask, emissive-alpha, multitexture and dissolve materials on their native path.
pub(crate) fn instance_supported(
    instance: &ActorGpuInstance,
    color_mask: bool,
    multitexture: bool,
) -> bool {
    let kind = instance.material & EntityRenderMaterialState::KIND_MASK;
    !color_mask
        && !multitexture
        && instance.material & render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG == 0
        && kind != EntityRenderMaterial::DissolveDepth as u32
        && kind != EntityRenderMaterial::DissolveColor as u32
        && !crate::actor::material::state(instance.material)
            .is_some_and(|state| state.emissive || state.depth_always || state.additive_alpha)
}

/// Checks the current frame before native queues can be suppressed.
pub(crate) fn frame_supported(
    actors: Option<&ActorRenderFrame>,
    hand: Option<&HandRigScene>,
    blocks: Option<&BlockEntityFrame>,
) -> bool {
    let actors_ok = actors.is_none_or(|frame| {
        frame
            .rig
            .instances
            .iter()
            .enumerate()
            .all(|(index, instance)| {
                let page = frame.instance_pages.get(index).copied().unwrap_or(0);
                if page == 0 {
                    return instance_supported(instance, false, false);
                }
                frame
                    .artwork
                    .pages
                    .get(usize::from(page) - 1)
                    .is_some_and(|page| {
                        instance_supported(instance, page.color_mask, page.multitexture)
                    })
            })
    });
    let hand_ok = hand
        .and_then(|scene| scene.frame.as_ref())
        .is_none_or(|frame| {
            frame
                .rig
                .instances
                .iter()
                .all(|instance| instance_supported(instance, false, false))
        });
    let blocks_ok = blocks.is_none_or(|frame| {
        frame.additive.is_empty()
            && frame
                .overlay
                .iter()
                .all(|vertex| vertex.actor_light != u32::MAX)
    });
    actors_ok && hand_ok && blocks_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emissive_alpha_materials_keep_native_coverage_and_lighting() {
        let instance = ActorGpuInstance {
            material: EntityRenderMaterial::Default.word(EntityRenderMaterialState {
                emissive: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!instance_supported(&instance, false, false));
        let mut frame = ActorRenderFrame::default();
        frame.rig.instances = std::sync::Arc::from([instance]);
        assert!(!frame_supported(Some(&frame), None, None));
    }

    #[test]
    fn native_texture_masks_and_paired_depth_materials_keep_native_composition() {
        let mut instance = ActorGpuInstance::default();
        assert!(instance_supported(&instance, false, false));
        assert!(!instance_supported(&instance, true, false));
        assert!(!instance_supported(&instance, false, true));
        for material in [
            EntityRenderMaterial::DissolveDepth as u32,
            EntityRenderMaterial::DissolveColor as u32,
            render_model::ACTOR_COLOR_MASK_MATERIAL_FLAG,
        ] {
            instance.material = material;
            assert!(!instance_supported(&instance, false, false));
        }
    }

    #[test]
    fn color_mask_and_multitexture_artwork_keep_the_native_frame() {
        let mut frame = ActorRenderFrame::default();
        frame.rig.instances = std::sync::Arc::from([ActorGpuInstance::default()]);
        frame.instance_pages = std::sync::Arc::from([1]);
        for (color_mask, multitexture) in [(true, false), (false, true)] {
            let page = render_model::ActorTexturePage {
                width: 1,
                height: 1,
                layers: 1,
                rgba8: std::sync::Arc::from([255; 4]),
                color_mask,
                multitexture,
            };
            let mut artwork = render_model::ActorArtworkPages::default();
            artwork.pages = std::sync::Arc::from([page]);
            frame.artwork = std::sync::Arc::new(artwork);
            assert!(!frame_supported(Some(&frame), None, None));
        }
    }

    #[test]
    fn beams_and_dragon_rays_stay_on_their_native_material_paths() {
        let mut frame = BlockEntityFrame::default();
        assert!(frame_supported(None, None, Some(&frame)));
        frame.overlay = std::sync::Arc::from([render_model::BlockEntityVertex {
            actor_light: u32::MAX,
            ..Default::default()
        }]);
        assert!(!frame_supported(None, None, Some(&frame)));
        frame.overlay = std::sync::Arc::from([]);
        frame.additive = std::sync::Arc::from([render_model::BlockEntityVertex::default()]);
        assert!(!frame_supported(None, None, Some(&frame)));
    }
}
