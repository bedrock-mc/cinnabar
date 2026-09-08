use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn monotonic_light_faces(
        &self,
        key: SubChunkKey,
        replacement: &SubChunkLight,
        replacement_direct: &DirectSkyMask,
    ) -> [bool; 6] {
        let Some(previous) = self.light_store.light(key) else {
            return [false; 6];
        };
        let previous_direct = match self.direct_sky.get(&key) {
            Some(direct) if direct.light_revision == previous.generation() => Some(&direct.mask),
            Some(_) => return [false; 6],
            None if previous.channel(LightChannel::Sky).is_uniform()
                && previous.get(LightChannel::Sky, 0, 0, 0) == Some(0) =>
            {
                None
            }
            None => return [false; 6],
        };
        let uniform_channels =
            [LightChannel::Block, LightChannel::Sky]
                .into_iter()
                .all(|channel| {
                    previous.channel(channel).is_uniform()
                        && replacement.channel(channel).is_uniform()
                });
        let uniform_direct = match (
            previous_direct.map(|direct| direct.as_ref()),
            replacement_direct,
        ) {
            (None | Some(DirectSkyMask::Uniform(false)), DirectSkyMask::Uniform(_)) => Some(true),
            (Some(DirectSkyMask::Uniform(true)), DirectSkyMask::Uniform(replacement)) => {
                Some(*replacement)
            }
            _ => None,
        };
        if uniform_channels && let Some(direct_is_monotonic) = uniform_direct {
            let levels_are_monotonic =
                [LightChannel::Block, LightChannel::Sky]
                    .into_iter()
                    .all(|channel| {
                        previous
                            .get(channel, 0, 0, 0)
                            .expect("uniform light sample is in range")
                            <= replacement
                                .get(channel, 0, 0, 0)
                                .expect("uniform light sample is in range")
                    });
            return [levels_are_monotonic && direct_is_monotonic; 6];
        }
        std::array::from_fn(|face| {
            face_cells(face).all(|(source, _)| {
                [LightChannel::Block, LightChannel::Sky]
                    .into_iter()
                    .all(|channel| {
                        previous
                            .get(channel, source[0], source[1], source[2])
                            .expect("face coordinates are in range")
                            <= replacement
                                .get(channel, source[0], source[1], source[2])
                                .expect("face coordinates are in range")
                    })
                    && !previous_direct.is_some_and(|direct| {
                        direct.get(source[0], source[1], source[2])
                            && !replacement_direct.get(source[0], source[1], source[2])
                    })
            })
        })
    }

    pub(in crate::stream) fn current_known_air_dominates_source_face(
        &self,
        source_key: SubChunkKey,
        destination_key: SubChunkKey,
        monotonic_faces: [bool; 6],
    ) -> bool {
        if !self.known_air.contains(&destination_key)
            || self.pending_light.contains_key(&destination_key)
            || self.in_flight_light.contains_key(&destination_key)
            || !self.light_is_current(destination_key)
        {
            return false;
        }
        let Some(face) = LIGHT_NEIGHBOUR_OFFSETS
            .into_iter()
            .position(|offset| offset_sub_chunk_key(source_key, offset) == Some(destination_key))
            .filter(|face| monotonic_faces[*face])
        else {
            return false;
        };
        let Some(source) = self.light_store.light(source_key) else {
            return false;
        };
        let Some(destination) = self.light_store.light(destination_key) else {
            return false;
        };
        let Some(source_direct) = self
            .direct_sky
            .get(&source_key)
            .filter(|direct| direct.light_revision == source.generation())
        else {
            return false;
        };
        let Some(destination_direct) = self
            .direct_sky
            .get(&destination_key)
            .filter(|direct| direct.light_revision == destination.generation())
        else {
            return false;
        };

        face_cells(face).all(|(source_position, destination_position)| {
            let get = |light: &SubChunkLight, channel, position: [u8; 3]| {
                light
                    .get(channel, position[0], position[1], position[2])
                    .expect("face coordinates are in range")
            };
            let source_block = get(source, LightChannel::Block, source_position);
            let destination_block = get(destination, LightChannel::Block, destination_position);
            if source_block.saturating_sub(1) > destination_block {
                return false;
            }

            let source_sky = get(source, LightChannel::Sky, source_position);
            let destination_sky = get(destination, LightChannel::Sky, destination_position);
            let carries_direct_sky = face == 2
                && source_sky == 15
                && source_direct.mask.get(
                    source_position[0],
                    source_position[1],
                    source_position[2],
                );
            let incoming_sky = if carries_direct_sky {
                15
            } else {
                source_sky.saturating_sub(1)
            };
            incoming_sky <= destination_sky
                && (!carries_direct_sky
                    || destination_direct.mask.get(
                        destination_position[0],
                        destination_position[1],
                        destination_position[2],
                    ))
        })
    }
}

fn face_cells(face: usize) -> impl Iterator<Item = ([u8; 3], [u8; 3])> {
    (0_u8..16).flat_map(move |u| {
        (0_u8..16).map(move |v| match face {
            0 => ([0, u, v], [15, u, v]),
            1 => ([15, u, v], [0, u, v]),
            2 => ([u, 0, v], [u, 15, v]),
            3 => ([u, 15, v], [u, 0, v]),
            4 => ([u, v, 0], [u, v, 15]),
            5 => ([u, v, 15], [u, v, 0]),
            _ => unreachable!("light face index is bounded"),
        })
    })
}
