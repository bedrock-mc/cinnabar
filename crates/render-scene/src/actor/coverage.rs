//! Final instance partition after model/page/layer ordering, with one dense main-pass prefix.
use super::*;

impl ActorRenderScene {
    /// Marks coverage-only actors after rig publication, preserving every payload correspondence.
    /// Bone addresses and relative layer order stay unchanged; final instance indices are reassigned.
    pub fn set_mask_only(&mut self, runtime_ids: Vec<u64>) -> &ActorRenderFrame {
        let frame = &mut self.frame;
        frame.mask_only_runtime_ids = runtime_ids.into();
        if frame.mask_only_runtime_ids.is_empty() {
            return frame;
        }
        let mut order: Vec<_> = (0..frame.rig.instances.len()).collect();
        order.sort_by_key(|index| {
            frame
                .mask_only_runtime_ids
                .contains(&frame.rig.manifest[*index].identity.runtime_id)
        });
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
