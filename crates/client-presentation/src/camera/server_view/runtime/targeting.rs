use super::*;

impl ServerCameraView {
    /// Prepares each stationary camera once so inactive target updates need no registry traversal.
    pub(super) fn prepare_presets(&mut self) {
        self.overrides = vec![PresetOverrides::default(); self.presets.len()];
        for index in 0..self.presets.len() {
            let declared = &self.presets[index];
            if preset_kind_from_name(&declared.name).is_some() {
                self.skips.invalid_options += u64::from(declared.radius.is_some())
                    + u64::from(declared.starting_rotation.is_some())
                    + u64::from(
                        declared.yaw_limit_min.is_some() || declared.yaw_limit_max.is_some(),
                    );
            }
            let Some(preset) = self.resolve_preset(index as u32) else {
                continue;
            };
            if preset.kind != Some(PresetKind::Free) {
                continue;
            }
            let invalid_horizontal = preset
                .horizontal_rotation_limit
                .is_some_and(|[a, b]| a < 0.0 || b < 0.0);
            let invalid_vertical = preset.vertical_rotation_limit.is_some_and(|[a, b]| a > b);
            self.skips.invalid_options +=
                u64::from(invalid_horizontal) + u64::from(invalid_vertical);
            self.overrides[index].stationary = Some(Pose {
                translation: Vec3::from_array(preset.position.map(|value| value.unwrap_or(0.0))),
                rotation: bedrock_rotation(
                    preset.rotation_degrees[1].unwrap_or(0.0),
                    preset.rotation_degrees[0].unwrap_or(0.0),
                ),
            });
            self.overrides[index].target_settings = TargetSettings {
                rotation_speed: preset.rotation_speed.unwrap_or(0.0),
                distance: preset.target_distance.unwrap_or(50.0),
                snap_to_target: preset.snap_to_target.unwrap_or(false),
                continue_targeting: preset.continue_targeting.unwrap_or(false),
                horizontal_limit: preset
                    .horizontal_rotation_limit
                    .filter(|_| !invalid_horizontal)
                    .unwrap_or([0.0, 360.0]),
                vertical_limit: preset
                    .vertical_rotation_limit
                    .filter(|_| !invalid_vertical)
                    .unwrap_or([0.0, 180.0]),
            };
        }
    }

    /// Returns targeting only for the selected stationary camera.
    pub(super) fn active_focus(&self) -> Option<TargetFocus> {
        self.overrides.get(self.active_preset_index?)?.focus
    }

    /// Target commands update every stationary camera, including currently inactive presets.
    pub(super) fn apply_target(
        &mut self,
        target: &protocol::CameraTargetInstruction,
        context: &ViewContext<'_>,
    ) {
        let active_pose = self.current_pose(context);
        let mut applied = false;
        for (index, state) in self.overrides.iter_mut().enumerate() {
            let Some(pose) = state.stationary else {
                continue;
            };
            let rotation = if self.active_preset_index == Some(index) {
                active_pose.rotation
            } else {
                state
                    .focus
                    .map_or(pose.rotation, |focus| focus.last_rotation())
            };
            state.focus = Some(TargetFocus::new(
                target.actor_unique_id,
                target.center_offset.map_or(Vec3::ZERO, Vec3::from_array),
                rotation,
                state.target_settings,
            ));
            applied = true;
        }
        if !applied {
            self.skips.invalid_options += 1;
        } else if (context.actors)(target.actor_unique_id).is_none() {
            self.skips.actor_bound += 1;
        }
    }

    /// Explicit removal affects the active target while inactive cameras retain their own state.
    pub(super) fn remove_target(&mut self, context: &ViewContext<'_>) {
        if let Some(index) = self.active_preset_index
            && let Some(focus) = self.active_focus()
        {
            let pose = self.current_pose(context);
            self.release_focus(
                index,
                focus.sample(pose.translation, (context.actors)(focus.actor)),
            );
        }
    }

    /// Observes attachment availability and advances every prepared stationary target.
    pub fn advance_target(&mut self, seconds: f32, context: &ViewContext<'_>) {
        self.observe_attachment(context);
        let active_position = self.current_pose(context).translation;
        let animated = self
            .spline
            .as_ref()
            .filter(|spline| !spline.is_finished())
            .map(SplinePlayback::sample);
        for index in 0..self.overrides.len() {
            let Some(mut focus) = self.overrides[index].focus else {
                continue;
            };
            let position = if self.active_preset_index == Some(index) {
                active_position
            } else {
                animated
                    .or(self.overrides[index].stationary)
                    .unwrap()
                    .translation
            };
            let previous = focus;
            if focus.advance(seconds, position, (context.actors)(focus.actor)) {
                if self.active_preset_index == Some(index) && focus.reanchored_since(previous) {
                    self.camera_reanchor_epoch = self.camera_reanchor_epoch.wrapping_add(1);
                }
                self.overrides[index].focus = Some(focus);
            } else {
                self.release_focus(index, focus.last_rotation());
            }
        }
    }

    /// A removed target preserves its last direction in that camera's instruction state.
    fn release_focus(&mut self, index: usize, rotation: Quat) {
        let state = &mut self.overrides[index];
        state.focus = None;
        state.rotation = Some(rotation);
        if let Some(pose) = &mut state.stationary {
            pose.rotation = rotation;
        }
        if self.active_preset_index == Some(index)
            && let Some(blend) = &mut self.pose
            && let Target::Fixed(pose) = &mut blend.to
        {
            pose.rotation = rotation;
        }
    }
}
