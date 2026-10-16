use super::*;

impl ServerCameraView {
    /// Returns only the selected rig's actor attachment.
    pub(super) fn active_attachment(&self) -> Option<i64> {
        self.overrides.get(self.active_preset_index?)?.attached
    }

    /// Updates every boom rig without selecting one or changing other camera kinds.
    pub(super) fn attach_to_entity(&mut self, actor: i64, context: &ViewContext<'_>) {
        let mut applied = false;
        for index in 0..self.presets.len() {
            if self.resolve_preset(index as u32).is_some_and(|preset| {
                matches!(
                    preset.base_name.as_deref(),
                    Some("minecraft:follow_orbit" | "minecraft:fixed_boom")
                )
            }) {
                self.overrides[index].attached = Some(actor);
                applied = true;
            }
        }
        if applied && (context.actors)(actor).is_none() {
            self.skips.actor_bound += 1;
        }
    }

    /// Releases the selected rig while inactive rigs retain their own attachments.
    pub(super) fn detach_from_entity(&mut self) {
        if let Some(index) = self.active_preset_index
            && let Some(state) = self.overrides.get_mut(index)
        {
            state.attached = None;
        }
        self.attachment_ready = false;
    }
}
