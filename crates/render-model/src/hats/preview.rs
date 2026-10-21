//! Authored attachment motion in the character preview's mirrored model coordinates.
use super::*;

impl HatRig {
    /// Samples repeating preview motion from caller-owned elapsed time, starting after any intro.
    pub fn preview_motion(&self, seconds: f64) -> Motion {
        Motion {
            seconds: self.playback.as_ref().map_or(0.0, |clip| {
                clip.normalize(f64::from(clip.rest_time()) + seconds.max(0.0))
            }),
            bend: 0.0,
        }
    }

    /// Poses the original mesh at its attachment pivot and assigns its owning preview part.
    pub fn preview_vertices(&self, motion: Motion) -> Vec<ActorVertex> {
        let poses = self.preview_pose(motion);
        self.geometry
            .vertices
            .iter()
            .map(|vertex| {
                let bone = vertex.bone_index as usize;
                let pose = poses[bone];
                let position = Vec3::from_slice(&pose.translation_scale[..3])
                    + Quat::from_array(pose.rotation)
                        * (Vec3::from_array(vertex.position)
                            - Vec3::from_array(self.geometry.bone_pivots[bone]));
                ActorVertex {
                    position: [-position.x, position.y, -position.z],
                    uv: vertex.uv,
                    part: self.preview_part,
                }
            })
            .collect()
    }

    /// Places an authored emitter in the same mirrored frame as the preview mesh.
    pub fn preview_emitter_transform(
        &self,
        index: usize,
        motion: Motion,
    ) -> Option<crate::ActorEmitterTransform> {
        let poses = self.preview_pose(motion);
        self.emitter_transform(
            index,
            &poses,
            &poses,
            [
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0, 0.0],
            ],
            1.0,
        )
    }

    /// Evaluates the shared attachment pose at its preview pivot.
    fn preview_pose(&self, motion: Motion) -> Arc<[RenderBoneTransform]> {
        let root = self.geometry.bone_pivots[self.head];
        self.pose(
            RenderBoneTransform {
                rotation: Quat::IDENTITY.to_array(),
                translation_scale: [root[0], root[1], root[2], 1.0],
                axis_scale: crate::UNIT_AXIS_SCALE,
            },
            motion,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_gallery_pixels_change_while_rigid_artwork_stays_still() {
        let [width, height] = thumbnail::SIZE;
        let wings = fixture::dragon_wings();
        let first = thumbnail::render_at(wings, width, height, 0.0).unwrap();
        let next = thumbnail::render_at(wings, width, height, 0.7).unwrap();
        assert_ne!(first.rgba8, next.rgba8);
        for image in [&first, &next] {
            assert!(
                image
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] != 0)
            );
            assert!(
                image
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 0)
            );
        }
        let crown = fixture::crown();
        assert_eq!(
            thumbnail::render_at(crown, width, height, 0.0)
                .unwrap()
                .rgba8,
            thumbnail::render_at(crown, width, height, 0.7)
                .unwrap()
                .rgba8,
        );
    }

    #[test]
    fn pet_previews_start_perched_and_continue_the_authored_cycle() {
        let parrot = rig(fixture::shoulder_parrot());
        let first = parrot.preview_vertices(parrot.preview_motion(0.0));
        assert_eq!(first.as_slice(), parrot.rest_vertices.as_ref());
        let playback = parrot.playback.as_ref().unwrap();
        let cycle = f64::from(playback.length - playback.rest_time());
        assert!((1..128).any(|frame| {
            parrot.preview_vertices(parrot.preview_motion(cycle * f64::from(frame) / 128.0))
                != first
        }));
        assert!(first.iter().all(|vertex| vertex.part == 1));
    }
}
