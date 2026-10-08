//! Sequential session palettes may omit carrier states and insert custom states.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Segment {
    wire_start: u32,
    len: u32,
    internal_start: u32,
    /// Custom states at or before this segment's start on the wire.
    shifted: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DensePalette {
    wire_to_internal: Box<[u32]>,
    internal_to_wire: Box<[u32]>,
}

/// Internal ids address the canonical carrier followed by session custom states.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SequentialIdRemap {
    segments: Vec<Segment>,
    palette: Option<DensePalette>,
}

impl SequentialIdRemap {
    /// Builds a remap from `(wire_start, len, internal_start)` custom runs.
    #[must_use]
    pub fn new(runs: impl IntoIterator<Item = (u32, u32, u32)>) -> Self {
        let mut runs = runs.into_iter().collect::<Vec<_>>();
        runs.sort_by_key(|run| run.0);
        let mut shifted = 0_u32;
        let segments = runs
            .into_iter()
            .map(|(wire_start, len, internal_start)| {
                shifted = shifted.saturating_add(len);
                Segment {
                    wire_start,
                    len,
                    internal_start,
                    shifted,
                }
            })
            .collect();
        Self {
            segments,
            palette: None,
        }
    }

    /// Builds a bounded session palette; omitted and unknown ids resolve to `u32::MAX`.
    /// The inverse selects the first wire entry when an internal id appears twice.
    #[must_use]
    pub fn from_palette(mut wire_to_internal: Vec<u32>, internal_state_count: u32) -> Self {
        let mut internal_to_wire = vec![u32::MAX; internal_state_count as usize];
        for (wire, internal) in wire_to_internal.iter_mut().enumerate() {
            if let Some(inverse) = internal_to_wire.get_mut(*internal as usize) {
                if *inverse == u32::MAX {
                    *inverse = wire as u32;
                }
            } else {
                *internal = u32::MAX;
            }
        }
        Self {
            segments: Vec::new(),
            palette: Some(DensePalette {
                wire_to_internal: wire_to_internal.into_boxed_slice(),
                internal_to_wire: internal_to_wire.into_boxed_slice(),
            }),
        }
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.palette.is_none() && self.segments.is_empty()
    }

    /// Translates a server block id to the id the carrier and registries use.
    #[must_use]
    pub fn to_internal(&self, wire: u32) -> u32 {
        if let Some(palette) = &self.palette {
            return palette
                .wire_to_internal
                .get(wire as usize)
                .copied()
                .unwrap_or(u32::MAX);
        }
        let after = self
            .segments
            .partition_point(|segment| segment.wire_start <= wire);
        let Some(segment) = after.checked_sub(1).map(|index| self.segments[index]) else {
            return wire;
        };
        let offset = wire - segment.wire_start;
        if offset < segment.len {
            segment.internal_start.saturating_add(offset)
        } else {
            wire - segment.shifted
        }
    }

    /// Recovers the wire id of a resolved internal state for diagnostics.
    #[must_use]
    pub fn to_wire(&self, internal: u32) -> u32 {
        if let Some(palette) = &self.palette {
            return palette
                .internal_to_wire
                .get(internal as usize)
                .copied()
                .unwrap_or(u32::MAX);
        }
        for segment in &self.segments {
            if let Some(offset) = internal.checked_sub(segment.internal_start)
                && offset < segment.len
            {
                return segment.wire_start.saturating_add(offset);
            }
        }
        let after = self.segments.partition_point(|segment| {
            segment
                .wire_start
                .saturating_sub(segment.shifted - segment.len)
                <= internal
        });
        let shifted = after
            .checked_sub(1)
            .map_or(0, |index| self.segments[index].shifted);
        internal.saturating_add(shifted)
    }
}

#[cfg(test)]
mod tests {
    use super::SequentialIdRemap;

    // Vanilla ids 0..10; custom runs of 2 at wire 3 and 1 at wire 9 (internal 10.., 12).
    #[test]
    fn vanilla_ids_shift_down_and_custom_runs_map_to_appended_ids() {
        let remap = SequentialIdRemap::new([(9, 1, 12), (3, 2, 10)]);
        let internal = (0..13)
            .map(|wire| remap.to_internal(wire))
            .collect::<Vec<_>>();
        assert_eq!(internal, [0, 1, 2, 10, 11, 3, 4, 5, 6, 12, 7, 8, 9]);
        assert!(SequentialIdRemap::default().is_identity());
        assert_eq!(SequentialIdRemap::default().to_internal(7), 7);
    }

    #[test]
    fn resolved_ids_recover_wire_ids_across_custom_runs() {
        let remap = SequentialIdRemap::new([(9, 1, 12), (3, 2, 10)]);
        for wire in 0..13 {
            assert_eq!(remap.to_wire(remap.to_internal(wire)), wire);
        }
        assert_eq!(SequentialIdRemap::default().to_wire(7), 7);
    }

    #[test]
    fn dense_palette_omits_unadvertised_internal_states() {
        let remap = SequentialIdRemap::from_palette(vec![0, 1, 4, 5], 6);
        assert_eq!(remap.to_internal(2), 4);
        assert_eq!(remap.to_wire(4), 2);
        assert_eq!(remap.to_wire(2), u32::MAX);
    }

    #[test]
    fn dense_palette_preserves_custom_insertion_after_omission() {
        let remap = SequentialIdRemap::from_palette(vec![0, 5, 1, 4], 6);
        for (wire, internal) in [0, 5, 1, 4].into_iter().enumerate() {
            assert_eq!(remap.to_internal(wire as u32), internal);
            assert_eq!(remap.to_wire(internal), wire as u32);
        }
    }

    #[test]
    fn dense_palette_bounds_unknown_wire_and_internal_ids() {
        let remap = SequentialIdRemap::from_palette(vec![0, 1, 4, 5], 6);
        assert_eq!(remap.to_internal(4), u32::MAX);
        assert_eq!(remap.to_internal(u32::MAX), u32::MAX);
        assert_eq!(remap.to_wire(6), u32::MAX);
        assert!(!remap.is_identity());
    }

    #[test]
    fn dense_palette_skips_invalid_internal_entries_and_keeps_first_inverse() {
        let remap = SequentialIdRemap::from_palette(vec![0, 7, 1, 1], 2);
        assert_eq!(remap.to_internal(1), u32::MAX);
        assert_eq!(remap.to_internal(3), 1);
        assert_eq!(remap.to_wire(1), 2);
    }
}
