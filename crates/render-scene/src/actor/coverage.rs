//! Final instance partition after model/page/layer ordering, with one dense main-pass prefix.
use super::*;

impl ActorRenderScene {
    /// Marks coverage-only actors after rig publication, preserving every payload correspondence.
    /// Bone addresses and relative layer order stay unchanged; final instance indices are reassigned.
    pub fn set_mask_only(&mut self, runtime_ids: Vec<u64>) -> &ActorRenderFrame {
        self.set_mask_only_except(runtime_ids, &[])
    }

    /// Keeps explicitly independent runtime/layer pairs in the main pass when their owner is masked.
    pub fn set_mask_only_except(
        &mut self,
        runtime_ids: Vec<u64>,
        independent: &[(u64, u8)],
    ) -> &ActorRenderFrame {
        let frame = &mut self.frame;
        frame.main_pass_instance_count = None;
        frame.mask_only_runtime_ids = runtime_ids.into();
        if frame.mask_only_runtime_ids.is_empty() {
            return frame;
        }
        let mask_only = |index: usize| {
            let identity = frame.rig.manifest[index].identity;
            frame.mask_only_runtime_ids.contains(&identity.runtime_id)
                && !independent.contains(&(identity.runtime_id, identity.layer))
        };
        let main_count = Some(
            (0..frame.rig.instances.len())
                .filter(|&index| !mask_only(index))
                .count() as u32,
        );
        let mut order: Vec<_> = (0..frame.rig.instances.len()).collect();
        order.sort_by_key(|index| mask_only(*index));
        frame.main_pass_instance_count = main_count;
        if order.iter().copied().eq(0..order.len()) {
            return frame;
        }
        frame.rig.instances = order
            .iter()
            .map(|index| frame.rig.instances[*index])
            .collect();
        frame.rig.manifest = order
            .iter()
            .enumerate()
            .map(|(new_index, old_index)| {
                let mut entry = frame.rig.manifest[*old_index].clone();
                entry.instance_index = new_index as u32;
                entry
            })
            .collect();
        frame.instances = order
            .iter()
            .map(|index| frame.instances[*index].clone())
            .collect();
        frame.instance_pages = order
            .iter()
            .map(|index| frame.instance_pages[*index])
            .collect();
        frame.instance_revision = frame.instance_revision.wrapping_add(1);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_partitions_owner_layers_after_its_independent_companion() {
        let mut scene = ActorRenderScene::default();
        let identity = render_model::ActorRenderIdentity {
            session_id: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
            ingress_sequence: 1,
            source_tick: Some(1),
            movement_revision: 1,
            pose_generation: render_api::PoseGeneration::new(1),
            layer: 0,
        };
        let layers = [0, render_model::hats::ACTOR_LAYER_HAT + 2, 3];
        let companion = layers[1];
        scene.frame.rig.manifest = layers
            .into_iter()
            .enumerate()
            .map(|(index, layer)| render_model::ActorDrawManifestEntry {
                identity: render_model::ActorRenderIdentity { layer, ..identity },
                rig: render_model::EntityRigId(0),
                completed_tick: 1,
                reset_generation: 1,
                route: render_model::ActorRigRoute::Compiled,
                instance_index: index as u32,
                previous_bone_base: index as u32,
                current_bone_base: index as u32,
                bone_count: 1,
            })
            .collect();
        scene.frame.rig.instances = (0..3)
            .map(|index| render_model::ActorGpuInstance {
                texture_layer: index,
                ..Default::default()
            })
            .collect();
        scene.frame.instances = (0..3)
            .map(|index| ActorRenderInstance {
                runtime_id: 7,
                position: [index as f32, 0.0, 0.0],
                pitch_radians: 0.0,
                yaw_radians: 0.0,
                head_yaw_radians: 0.0,
                skin_layer: index,
            })
            .collect();
        scene.frame.instance_pages = Arc::from([0, 1, 2]);
        let frame = scene.set_mask_only_except(vec![7], &[(7, companion)]);
        assert_eq!(frame.main_pass_instance_count, Some(1));
        assert_eq!(frame.rig.manifest[0].identity.layer, companion);
        assert_eq!(frame.rig.instances[0].texture_layer, 1);
        assert_eq!(frame.instances[0].skin_layer, 1);
        assert_eq!(frame.instance_pages[0], 1);
        for (index, entry) in frame.rig.manifest.iter().enumerate() {
            assert_eq!(entry.instance_index, index as u32);
        }
    }
}
