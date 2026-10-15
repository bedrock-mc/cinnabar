//! Ordered terrain units for ordinary instanced vertex-pulling draws.

use std::ops::Range;

/// Quads covered by one indexed instance, including its unused tail.
pub const QUADS_PER_UNIT: u32 = 64;

/// An allocation's contiguous stream fragment. Bounds are conservative section-local bounds;
/// visibility must also validate the allocation generation before using a retained descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainUnit {
    pub stream_start: u32,
    pub quad_count: u32,
    pub origin_slot: u32,
    pub bounds: [[i32; 3]; 2],
    /// Packed cube face number, or `None` for a stream without directional culling.
    pub facing: Option<u8>,
    pub allocation_generation: u64,
}

impl TerrainUnit {
    /// Splits a valid stream range without crossing an allocation or facing boundary.
    pub fn build(
        range: Range<u32>,
        origin_slot: u32,
        bounds: [[i32; 3]; 2],
        facing: Option<u8>,
        allocation_generation: u64,
    ) -> impl Iterator<Item = Self> {
        (range.start..range.end)
            .step_by(QUADS_PER_UNIT as usize)
            .map(move |stream_start| Self {
                stream_start,
                quad_count: (range.end - stream_start).min(QUADS_PER_UNIT),
                origin_slot,
                bounds,
                facing,
                allocation_generation,
            })
    }

    /// The immutable geometry address uploaded independently of its visibility.
    pub fn descriptor(&self) -> VisibleTerrainUnit {
        VisibleTerrainUnit {
            stream_start: self.stream_start,
            quad_count: self.quad_count,
            origin_slot: self.origin_slot,
            padding: 0,
        }
    }

    /// Appends visible descriptors in their original order; callers own allocation visibility.
    pub fn compact(
        units: &[Self],
        mut visible: impl FnMut(&Self) -> bool,
        output: &mut Vec<VisibleTerrainUnit>,
    ) {
        output.extend(
            units
                .iter()
                .filter(|unit| visible(unit))
                .map(Self::descriptor),
        );
    }
}

/// Shader-facing part of a CPU-selected unit. Each storage record matches a WGSL vec4.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VisibleTerrainUnit {
    pub stream_start: u32,
    pub quad_count: u32,
    pub origin_slot: u32,
    pub padding: u32,
}

impl VisibleTerrainUnit {
    /// Resolves a quad only after rejecting padding, before any geometry fetch.
    #[must_use]
    pub fn quad(self, vertex_index: u32) -> Option<(u32, u32)> {
        let quad = vertex_index / 4;
        (quad < self.quad_count).then(|| (self.stream_start + quad, vertex_index % 4))
    }
}

/// Consecutive shards that fit both the buffer binding and the draw's instance count.
pub fn shards(unit_count: usize, max_bytes: u64) -> impl Iterator<Item = Range<usize>> {
    let capacity =
        (max_bytes / size_of::<VisibleTerrainUnit>() as u64).min(u64::from(u32::MAX)) as usize;
    assert!(
        capacity > 0,
        "unit batching requires room for one descriptor"
    );
    (0..unit_count)
        .step_by(capacity)
        .map(move |start| start..start.saturating_add(capacity).min(unit_count))
}

/// One storage binding holds a header, stable descriptors, then ordered u32 references.
/// Reference words are packed four per vec4, matching the shader's storage element alignment.
#[derive(Clone, Copy, Debug)]
pub struct IndexedUnitPage {
    pub capacity: usize,
}

impl IndexedUnitPage {
    /// Largest slot count whose descriptors and references both fit the binding.
    pub fn capacity_for(max_bytes: u64) -> usize {
        let words = (max_bytes / size_of::<VisibleTerrainUnit>() as u64).min(u64::from(u32::MAX));
        let mut capacity = words.saturating_sub(1) * 4 / 5;
        if 1 + capacity + capacity.div_ceil(4) > words {
            capacity = capacity.saturating_sub(1);
        }
        capacity as usize
    }

    /// Byte offset of an ordered reference region, after the header and descriptor slots.
    pub fn reference_offset(self) -> u64 {
        (1 + self.capacity as u64) * size_of::<VisibleTerrainUnit>() as u64
    }

    /// Binding size including the final partially used reference vec4.
    pub fn byte_size(self) -> u64 {
        self.reference_offset()
            + self.capacity.div_ceil(4) as u64 * size_of::<VisibleTerrainUnit>() as u64
    }

    /// The shader locates references through this header before fetching a descriptor.
    pub fn header(self) -> [u32; 4] {
        [self.capacity as u32 + 1, 0, 0, 0]
    }
}

#[cfg(test)]
mod tests;
