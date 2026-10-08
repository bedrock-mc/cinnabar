use super::*;
use crate::chunk::transparent::face_metric::TransparentFaceMetric;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MixedStream {
    Model,
    Water,
}

pub(super) struct MixedFace {
    pub(super) stream: MixedStream,
    pub(super) index: u32,
    pub(super) centroid: Vec3,
    pub(super) stable: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MixedTerrainSegment {
    pub(super) stream: MixedStream,
    pub(super) range: Range<u32>,
}

pub(super) fn merge_faces(
    key: SubChunkKey,
    camera: Vec3,
    mut faces: Vec<MixedFace>,
    segment_limit: usize,
) -> Option<Vec<MixedTerrainSegment>> {
    let metric = TransparentFaceMetric::new(camera).for_chunk(key);
    faces.sort_by(|left, right| {
        metric
            .distance(right.centroid)
            .total_cmp(&metric.distance(left.centroid))
            .then_with(|| left.stream.cmp(&right.stream))
            .then_with(|| left.stable.cmp(&right.stable))
    });
    let mut segments = Vec::<MixedTerrainSegment>::new();
    for face in faces {
        if let Some(last) = segments.last_mut()
            && last.stream == face.stream
            && last.range.end == face.index
        {
            last.range.end = face.index.checked_add(1)?;
            continue;
        }
        if segments.len() == segment_limit {
            return None;
        }
        segments.push(MixedTerrainSegment {
            stream: face.stream,
            range: face.index..face.index.checked_add(1)?,
        });
    }
    Some(segments)
}

pub(super) fn collect_faces(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
    model_words: &[[u32; 2]],
    assets: &ChunkTextureAssets,
    water_refs: &[PackedTransparentDrawRef],
) -> Option<Vec<MixedFace>> {
    let model_base = allocation.model_range.as_ref()?.start.checked_div(4)?;
    let liquid_base = allocation.liquid_range.as_ref()?.start.checked_div(4)?;
    let mut faces = Vec::with_capacity(model_words.len().checked_add(water_refs.len())?);
    let transparent_end = instance
        .depth_liquid_start
        .map_or(instance.liquid_quads.len(), |index| index as usize);
    for (index, draw) in water_refs.iter().enumerate() {
        if draw.metadata_index() != allocation.metadata_index {
            return None;
        }
        let local = usize::try_from(draw.liquid_record_index().checked_sub(liquid_base)?).ok()?;
        let quad = *instance.liquid_quads.get(..transparent_end)?.get(local)?;
        faces.push(MixedFace {
            stream: MixedStream::Water,
            index: u32::try_from(index).ok()?,
            centroid: Vec3::from_array(liquid_quad_centroid(instance.origin, quad)),
            stable: [u32::try_from(local).ok()?, 0],
        });
    }
    for (index, &words) in model_words.iter().enumerate() {
        let local = words[0].checked_sub(model_base)?;
        let (centroid, expected) = transparent_model_draw_candidate(
            instance.key,
            &instance.model_refs,
            PackedModelDrawRef::new(local, words[1]),
            assets.assets().model_templates(),
            assets.assets().model_quads(),
            model_base,
        )?;
        if expected != words {
            return None;
        }
        faces.push(MixedFace {
            stream: MixedStream::Model,
            index: u32::try_from(index).ok()?,
            centroid,
            stable: [local, words[1]],
        });
    }
    Some(faces)
}
