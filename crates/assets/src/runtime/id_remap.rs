//! Wire-to-internal block id translation for sequential sessions whose custom
//! blocks sort among vanilla names and so shift the vanilla wire ids.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Segment {
    wire_start: u32,
    len: u32,
    internal_start: u32,
    /// Custom states at or before this segment's start on the wire.
    shifted: u32,
}

/// Internal ids are vanilla ids followed by the session's custom states.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SequentialIdRemap {
    segments: Vec<Segment>,
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
        Self { segments }
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.segments.is_empty()
    }

    /// Translates a server block id to the id the carrier and registries use.
    #[must_use]
    pub fn to_internal(&self, wire: u32) -> u32 {
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
}
