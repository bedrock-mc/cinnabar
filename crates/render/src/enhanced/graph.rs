use super::{
    hand_layer::{EnhancedHandCompositeLabel, enhanced_hand_composite},
    post::{EnhancedPostLabel, enhanced_post},
    shadows::{EnhancedShadowLabel, enhanced_shadows},
    snapshot::{EnhancedSnapshotLabel, enhanced_snapshot},
};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    prelude::*,
};

#[derive(Resource)]
struct PassesInstalled;

/// Keeps the enhanced world grade before the hand layer and HUD.
pub(super) fn install_graph(world: &mut World) {
    if world.contains_resource::<PassesInstalled>() {
        return;
    }
    let hand = crate::viewmodel_render::enhanced_post_pass(world);
    let rig = crate::hand_rig_render::enhanced_post_pass(world);
    let installed = world
        .try_schedule_scope(Core3d, |_, schedule| {
            use crate::{RuntimeStage, gpu_timing::profiled};
            use bevy::core_pipeline::tonemapping::tonemapping;
            schedule.add_systems(
                (
                    profiled(
                        enhanced_shadows,
                        Some(RuntimeStage::GpuShadows),
                        "EnhancedShadowLabel",
                    )
                    .in_set(EnhancedShadowLabel)
                    .before(crate::scene_target::ScenePass::Opaque),
                    profiled(
                        enhanced_snapshot,
                        Some(RuntimeStage::GpuBlit),
                        "EnhancedSnapshotLabel",
                    )
                    .in_set(EnhancedSnapshotLabel)
                    .after(crate::scene_target::ScenePass::Opaque)
                    .after(crate::chunk::GpuCullLateLabel)
                    .after(crate::entity_shadow_render::EntityShadowLabel)
                    .before(crate::scene_target::ScenePass::Transparent),
                )
                    .in_set(Core3dSystems::MainPass),
            );
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
