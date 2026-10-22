//! Large buffers compiled outputs own, so a cache can weigh what keeping them costs.

use std::{collections::HashSet, sync::Arc};

/// A visitor receiving each buffer's address and byte length.
pub type BufferVisitor<'a> = dyn FnMut(*const u8, usize) + 'a;

/// Lets a cache weigh a compiled output by the large buffers it owns. A buffer shared between
/// outputs reports the same address, so [`retained_bytes`] counts it once.
pub trait RetainedBuffers {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>);
}

/// Reports `slice` as one buffer.
pub fn visit_slice<T>(slice: &[T], visit: &mut BufferVisitor<'_>) {
    visit(slice.as_ptr().cast(), size_of_val(slice));
}

/// Bytes of the distinct non-empty buffers `outputs` own.
pub fn retained_bytes(outputs: &[&dyn RetainedBuffers]) -> usize {
    let mut seen = HashSet::new();
    let mut total = 0usize;
    for output in outputs {
        output.visit_buffers(&mut |address, bytes| {
            if bytes != 0 && seen.insert(address as usize) {
                total = total.saturating_add(bytes);
            }
        });
    }
    total
}

impl<T: RetainedBuffers + ?Sized> RetainedBuffers for Arc<T> {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        T::visit_buffers(self, visit);
    }
}

impl<T: RetainedBuffers + ?Sized> RetainedBuffers for Box<T> {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        T::visit_buffers(self, visit);
    }
}

impl<T: RetainedBuffers> RetainedBuffers for Option<T> {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        if let Some(output) = self {
            output.visit_buffers(visit);
        }
    }
}

impl<T: RetainedBuffers> RetainedBuffers for [T] {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        for output in self {
            output.visit_buffers(visit);
        }
    }
}

impl<T: RetainedBuffers, const N: usize> RetainedBuffers for [T; N] {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        self.as_slice().visit_buffers(visit);
    }
}

impl<T: RetainedBuffers> RetainedBuffers for Vec<T> {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        self.as_slice().visit_buffers(visit);
    }
}

impl RetainedBuffers for crate::ActorTexture {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            source: _,
            width: _,
            height: _,
            pixel_sha256: _,
            rgba8,
        } = self;
        visit_slice(rgba8, visit);
    }
}

impl RetainedBuffers for crate::SessionEntityPack {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            assets,
            textures,
            bindings,
            equipment,
        } = self;
        assets.visit_buffers(visit);
        textures.visit_buffers(visit);
        visit_slice(bindings, visit);
        for binding in bindings.iter() {
            visit_slice(binding.material.as_bytes(), visit);
        }
        equipment.visit_buffers(visit);
    }
}

impl RetainedBuffers for crate::texture::TextureArray {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self { layers: _, mips } = self;
        for crate::texture::TextureMip { size: _, rgba8 } in mips {
            visit_slice(rgba8, visit);
        }
    }
}

impl RetainedBuffers for crate::CellGlyph {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        let Self {
            codepoint: _,
            size: _,
            rgba8,
            bearing: _,
            advance_64: _,
            draw_size_64: _,
        } = self;
        visit_slice(rgba8, visit);
    }
}

impl RetainedBuffers for crate::BlockOverlay {
    fn visit_buffers(&self, visit: &mut BufferVisitor<'_>) {
        // Visual, material and model tables are small beside the texture shards.
        let Self {
            visuals: _,
            carried_cube_faces: _,
            light_properties: _,
            materials: _,
            model_templates: _,
            model_random_offsets: _,
            model_quads,
            animations: _,
            animation_frames: _,
            textures,
            texture_source_sizes: _,
            texture_source_grids: _,
            hashes: _,
            material_overrides: _,
            face_overrides: _,
            biomes,
            max_page_layers: _,
        } = self;
        visit_slice(model_quads, visit);
        textures.visit_buffers(visit);
        if let Some(crate::CompiledBiomeAssets {
            tint_maps_rgb8,
            rules: _,
        }) = biomes
        {
            visit_slice(tint_maps_rgb8, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Outputs that share one raster must not count it twice, or a kept stack looks twice its size.
    #[test]
    fn shared_buffers_count_once_and_empty_ones_not_at_all() {
        let texture = |rgba8: &Arc<[u8]>| crate::ActorTexture {
            source: 0,
            width: 1,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: Arc::clone(rgba8),
        };
        let shared: Arc<[u8]> = Arc::from(vec![1; 64]);
        let other: Arc<[u8]> = Arc::from(vec![2; 16]);
        let empty: Arc<[u8]> = Arc::from(Vec::new());
        let first = vec![texture(&shared), texture(&other)];
        let second = Some(Arc::new(vec![texture(&shared), texture(&empty)]));
        assert_eq!(retained_bytes(&[&first, &second]), 80);
    }
}
