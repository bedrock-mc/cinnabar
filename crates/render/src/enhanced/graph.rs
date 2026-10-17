use super::{
    depth::{EnhancedDepthLabel, enhanced_depth},
    hand_layer::{EnhancedHandCompositeLabel, enhanced_hand_composite},
    post::{
        EnhancedLightingLabel, EnhancedPostLabel, EnhancedSkyLabel, enhanced_lighting,
        enhanced_post, enhanced_sky,
    },
    shadows::{EnhancedShadowLabel, enhanced_shadows},
    snapshot::{EnhancedSnapshotLabel, enhanced_snapshot},
};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
};

#[derive(Resource)]
struct PassesInstalled;

/// Keeps shadows, depth and lighting before the world, the sky before the opaque snapshot,
/// and the enhanced world grade before the hand layer and HUD.
pub(super) fn install_graph(world: &mut World) {
    if world.contains_resource::<PassesInstalled>() {
        return;
    }
    let hand = crate::viewmodel_render::enhanced_post_pass(world);
    let rig = crate::hand_rig_render::enhanced_post_pass(world);
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            use bevy::core_pipeline::tonemapping::tonemapping;
            use {crate::gpu_timing::profiled, render_scene::RuntimeStage};
            schedule.configure_sets(
                (
                    EnhancedShadowLabel,
                    EnhancedDepthLabel,
                    EnhancedLightingLabel,
                )
                    .chain()
                    .before(crate::scene_target::ScenePass::Opaque)
                    .in_set(Core3dSystems::MainPass),
            );
            schedule.configure_sets(
                (EnhancedSkyLabel, EnhancedSnapshotLabel)
                    .chain()
                    .after(crate::scene_target::ScenePass::Opaque)
                    .before(crate::scene_target::ScenePass::Transparent)
                    .in_set(Core3dSystems::MainPass),
            );
            schedule.add_systems((
                profiled(
                    enhanced_shadows,
                    Some(RuntimeStage::GpuShadows),
                    "EnhancedShadowLabel",
                )
                .in_set(EnhancedShadowLabel),
                profiled(enhanced_depth, None, "EnhancedDepthLabel").in_set(EnhancedDepthLabel),
                profiled(enhanced_lighting, None, "EnhancedLightingLabel")
                    .in_set(EnhancedLightingLabel),
                profiled(enhanced_sky, None, "EnhancedSkyLabel").in_set(EnhancedSkyLabel),
                profiled(
                    enhanced_snapshot,
                    Some(RuntimeStage::GpuBlit),
                    "EnhancedSnapshotLabel",
                )
                .in_set(EnhancedSnapshotLabel)
                .after(crate::chunk::GpuCullLateLabel)
                .after(crate::entity_shadow_render::EntityShadowLabel),
            ));
            schedule.configure_sets(
                (
                    EnhancedPostLabel,
                    EnhancedHandLabel,
                    EnhancedHandRigLabel,
                    EnhancedHandCompositeLabel,
                )
                    .chain()
                    .after(bevy::post_process::bloom::bloom)
                    .before(tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
            schedule.add_systems((
                profiled(
                    enhanced_post,
                    Some(RuntimeStage::GpuPost),
                    "EnhancedPostLabel",
                )
                .in_set(EnhancedPostLabel),
                profiled(
                    enhanced_hand_composite,
                    Some(RuntimeStage::GpuHand),
                    "EnhancedHandCompositeLabel",
                )
                .in_set(EnhancedHandCompositeLabel),
            ));
            if let Some(hand) = hand {
                schedule.add_systems(hand.in_set(EnhancedHandLabel));
            }
            if let Some(rig) = rig {
                schedule.add_systems(rig.in_set(EnhancedHandRigLabel));
            }
        })
        .is_ok();
    if installed {
        world.insert_resource(PassesInstalled);
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(super) struct EnhancedHandLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub(super) struct EnhancedHandRigLabel;
