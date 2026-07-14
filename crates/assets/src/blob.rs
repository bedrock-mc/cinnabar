use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use crate::model::{MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAGS_MASK};
use crate::{
    AssetError, BlockFlags, CompiledAssets, DIAGNOSTIC_MATERIAL, MAX_ANIMATION_FRAMES,
    MAX_ANIMATIONS, MAX_MATERIALS, MAX_MODEL_QUADS, MAX_MODEL_TEMPLATES, MAX_TEXTURE_LAYERS,
    MAX_TEXTURE_PAGES, MIP_COUNT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_STAIR, NO_ANIMATION, NO_MODEL_TEMPLATE, TILE_SIZE, TextureRef, VisualKind,
    biome::{TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE, validate_biome_assets},
    compiler::{material_flags_are_valid, visual_semantics_are_valid},
    model::{ANIMATION_FLAGS_MASK, model_quad_flags_are_valid},
};

pub const BLOB_MAGIC: [u8; 8] = *b"MCBEAS04";
pub const BLOB_VERSION: u32 = 4;
pub(crate) const HEADER_BYTES: usize = 200;
pub(crate) const HASH_BYTES: usize = 32;
pub(crate) const VISUAL_BYTES: usize = 40;
pub(crate) const HASH_ENTRY_BYTES: usize = 8;
pub(crate) const MATERIAL_BYTES: usize = 12;
pub(crate) const TEMPLATE_BYTES: usize = 12;
pub(crate) const QUAD_BYTES: usize = 48;
pub(crate) const ANIMATION_BYTES: usize = 28;
pub(crate) const FRAME_BYTES: usize = 4;
pub(crate) const PAGE_BYTES: usize = 64;
pub(crate) const BIOME_RULE_BYTES: usize = 36;
pub(crate) const MAX_VISUALS: usize = 65_536;

/// Serializes canonical, bounded `MCBEAS04` compiler output with a trailing SHA-256.
pub fn encode_blob(compiled: &CompiledAssets) -> Result<Box<[u8]>, AssetError> {
    validate_compiled(compiled)?;
    let sizes = [
        size(compiled.visuals.len(), VISUAL_BYTES, "visual")?,
        size(compiled.hashed.len(), HASH_ENTRY_BYTES, "hash")?,
        size(compiled.materials.len(), MATERIAL_BYTES, "material")?,
        size(
            compiled.model_templates.len(),
            TEMPLATE_BYTES,
            "model template",
        )?,
        size(compiled.model_quads.len(), QUAD_BYTES, "model quad")?,
        size(compiled.animations.len(), ANIMATION_BYTES, "animation")?,
        size(
            compiled.animation_frames.len(),
            FRAME_BYTES,
            "animation frame",
        )?,
        size(compiled.texture_pages.len(), PAGE_BYTES, "texture page")?,
    ];
    let texture_bytes = compiled
        .texture_pages
        .iter()
        .try_fold(0usize, |sum, page| {
            add(sum, texture_length(&page.texture)?, "texture payload")
        })?;
    let biome_rule_bytes = size(compiled.biomes.rules.len(), BIOME_RULE_BYTES, "biome rule")?;
    let biome_name_bytes = compiled
        .biomes
        .rules
        .iter()
        .try_fold(0usize, |sum, rule| add(sum, rule.name.len(), "biome names"))?;
    let section_sizes = [
        sizes[0],
        sizes[1],
        sizes[2],
        sizes[3],
        sizes[4],
        sizes[5],
        sizes[6],
        sizes[7],
        texture_bytes,
        TINT_MAP_BYTES,
        biome_rule_bytes,
        biome_name_bytes,
    ];
    let mut offsets = [0usize; 13];
    offsets[0] = HEADER_BYTES;
    for (index, length) in section_sizes.iter().copied().enumerate() {
        offsets[index + 1] = add(offsets[index], length, "blob section")?;
    }
    let payload_length = offsets[12];
    let total_length = add(payload_length, HASH_BYTES, "blob hash")?;
    let mut bytes = Vec::with_capacity(total_length);
    bytes.extend_from_slice(&BLOB_MAGIC);
    for value in [
        BLOB_VERSION,
        TILE_SIZE,
        MIP_COUNT,
        count(compiled.visuals.len(), "visual count")?,
        count(compiled.hashed.len(), "hash count")?,
        count(compiled.materials.len(), "material count")?,
        count(compiled.model_templates.len(), "template count")?,
        count(compiled.model_quads.len(), "quad count")?,
        count(compiled.animations.len(), "animation count")?,
        count(compiled.animation_frames.len(), "frame count")?,
        count(compiled.texture_pages.len(), "page count")?,
        TINT_MAP_COUNT as u32,
        TINT_MAP_SIZE,
        count(compiled.biomes.rules.len(), "biome count")?,
    ] {
        push_u32(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0; 32]);
    for offset in offsets {
        push_offset(&mut bytes, offset, "section offset")?;
    }
    debug_assert_eq!(bytes.len(), HEADER_BYTES);

    for visual in &compiled.visuals {
        for material in visual.faces {
            push_u32(&mut bytes, material);
        }
        bytes.push(visual.flags.bits());
        bytes.push(visual.kind as u8);
        bytes.push(visual.contributor_role as u8);
        bytes.push(0);
        push_u32(&mut bytes, visual.model_template);
        push_u32(&mut bytes, visual.animation);
        push_u32(&mut bytes, visual.variant);
    }
    for &(hash, visual) in &compiled.hashed {
        push_u32(&mut bytes, hash);
        push_u32(&mut bytes, visual);
    }
    for material in &compiled.materials {
        push_u32(&mut bytes, material.texture.raw());
        push_u32(&mut bytes, material.flags);
        push_u32(&mut bytes, material.animation);
    }
    for template in &compiled.model_templates {
        push_u32(&mut bytes, template.quad_start);
        push_u32(&mut bytes, template.quad_count);
        push_u32(&mut bytes, template.flags);
    }
    for quad in &compiled.model_quads {
        for position in quad.positions {
            for coordinate in position {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        for uv in quad.uvs {
            for coordinate in uv {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        push_u32(&mut bytes, quad.material);
        push_u32(&mut bytes, quad.flags);
    }
    for animation in &compiled.animations {
        push_u32(&mut bytes, animation.frame_start);
        push_u32(&mut bytes, animation.frame_count);
        push_u32(&mut bytes, animation.ticks_per_frame);
        push_u32(&mut bytes, animation.atlas_index);
        push_u32(&mut bytes, animation.atlas_tile_variant);
        push_u32(&mut bytes, animation.replicate);
        push_u32(&mut bytes, animation.flags);
    }
    for frame in &compiled.animation_frames {
        push_u32(&mut bytes, frame.raw());
    }

    let mut texture_offset = offsets[8];
    for (index, page) in compiled.texture_pages.iter().enumerate() {
        let length = texture_length(&page.texture)?;
        let digest = texture_digest(&page.texture);
        push_u32(&mut bytes, index as u32);
        push_u32(&mut bytes, page.texture.layers);
        push_u32(&mut bytes, MIP_COUNT);
        push_u32(&mut bytes, 0);
        push_offset(&mut bytes, texture_offset, "page payload offset")?;
        push_offset(&mut bytes, length, "page payload length")?;
        bytes.extend_from_slice(&digest);
        texture_offset = add(texture_offset, length, "page payload")?;
    }
    debug_assert_eq!(bytes.len(), offsets[8]);
    for page in &compiled.texture_pages {
        for mip in &page.texture.mips {
            bytes.extend_from_slice(&mip.rgba8);
        }
    }
    bytes.extend_from_slice(&compiled.biomes.tint_maps_rgb8);
    let mut name_offset = 0usize;
    for rule in &compiled.biomes.rules {
        push_u32(&mut bytes, rule.id);
        push_u32(&mut bytes, count(name_offset, "biome name offset")?);
        bytes.extend_from_slice(
            &u16::try_from(rule.name.len())
                .map_err(|_| AssetError::BlobSizeOverflow {
                    section: "biome name length",
                })?
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&rule.flags.to_le_bytes());
        for source in [rule.grass, rule.foliage, rule.dry_foliage, rule.water] {
            push_u32(&mut bytes, source.raw());
        }
        push_u32(&mut bytes, rule.temperature_bits);
        push_u32(&mut bytes, rule.downfall_bits);
        name_offset = add(name_offset, rule.name.len(), "biome names")?;
    }
    for rule in &compiled.biomes.rules {
        bytes.extend_from_slice(rule.name.as_bytes());
    }
    debug_assert_eq!(bytes.len(), payload_length);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes.into_boxed_slice())
}

fn validate_compiled(compiled: &CompiledAssets) -> Result<(), AssetError> {
    validate_biome_assets(&compiled.biomes)?;
    bounded("visual", compiled.visuals.len(), MAX_VISUALS)?;
    bounded("hash", compiled.hashed.len(), MAX_VISUALS)?;
    if compiled.materials.len() > MAX_MATERIALS {
        return Err(AssetError::TooManyMaterials {
            count: compiled.materials.len(),
            max: MAX_MATERIALS,
        });
    }
    bounded(
        "model template",
        compiled.model_templates.len(),
        MAX_MODEL_TEMPLATES,
    )?;
    bounded("model quad", compiled.model_quads.len(), MAX_MODEL_QUADS)?;
    bounded("animation", compiled.animations.len(), MAX_ANIMATIONS)?;
    bounded(
        "animation frame",
        compiled.animation_frames.len(),
        MAX_ANIMATION_FRAMES,
    )?;
    if compiled.texture_pages.is_empty() || compiled.texture_pages.len() > MAX_TEXTURE_PAGES {
        return Err(invalid("texture page count must be one or two"));
    }
    for (page_index, page) in compiled.texture_pages.iter().enumerate() {
        if page.texture.layers as usize > MAX_TEXTURE_LAYERS {
            return Err(AssetError::TooManyTextureLayers {
                count: page.texture.layers as usize,
                max: MAX_TEXTURE_LAYERS,
                key: None,
                path: None,
            });
        }
        validate_texture(page_index, &page.texture)?;
    }
    let diagnostic = compiled
        .materials
        .first()
        .ok_or_else(|| invalid("missing diagnostic material"))?;
    if diagnostic.texture != TextureRef::DIAGNOSTIC
        || diagnostic.flags != 0
        || diagnostic.animation != NO_ANIMATION
    {
        return Err(invalid("material 0 is not canonical diagnostic material"));
    }
    for (index, material) in compiled.materials.iter().enumerate() {
        validate_texture_ref(material.texture, &compiled.texture_pages, "material")?;
        if !material_flags_are_valid(material.flags) {
            return Err(invalid(format!("material {index} has unsupported flags")));
        }
        optional_id(
            material.animation,
            compiled.animations.len(),
            "material animation",
        )?;
    }
    let compound_tails = compiled_compound_tails(&compiled.model_templates)?;
    let stair_bases = compiled_stair_bases(&compiled.model_templates)?;
    let mut referenced_stair_bases = vec![false; stair_bases.len()];
    for (index, visual) in compiled.visuals.iter().enumerate() {
        if BlockFlags::from_bits(visual.flags.bits())
            .is_none_or(|flags| !flags.has_valid_semantics())
        {
            return Err(invalid(format!("visual {index} has invalid flags")));
        }
        if !visual_semantics_are_valid(visual.kind, visual.flags, visual.contributor_role) {
            return Err(invalid(format!(
                "visual {index} kind, flags, and contributor role disagree"
            )));
        }
        if visual
            .faces
            .iter()
            .any(|&id| id as usize >= compiled.materials.len())
        {
            return Err(invalid(format!(
                "visual {index} references invalid material"
            )));
        }
        optional_id(
            visual.model_template,
            compiled.model_templates.len(),
            "visual template",
        )?;
        if visual.model_template != NO_MODEL_TEMPLATE
            && compound_tails[visual.model_template as usize]
        {
            return Err(invalid(
                "compound continuation cannot be directly visual-referenced",
            ));
        }
        optional_id(
            visual.animation,
            compiled.animations.len(),
            "visual animation",
        )?;
        match visual.kind {
            VisualKind::Diagnostic if visual.model_template != NO_MODEL_TEMPLATE => {
                return Err(invalid("diagnostic visual has model reference"));
            }
            VisualKind::Model | VisualKind::Cross if visual.model_template == NO_MODEL_TEMPLATE => {
                return Err(invalid("model visual has no template"));
            }
            VisualKind::Cube | VisualKind::Liquid | VisualKind::Invisible
                if visual.model_template != NO_MODEL_TEMPLATE =>
            {
                return Err(invalid("non-model visual has template"));
            }
            _ => {}
        }
        if visual.model_template != NO_MODEL_TEMPLATE
            && compiled.model_templates[visual.model_template as usize].flags
                & MODEL_TEMPLATE_FLAG_STAIR
                != 0
        {
            let Some(base_index) = stair_bases
                .iter()
                .position(|&base| base == visual.model_template as usize)
            else {
                return Err(invalid(
                    "stair visual does not reference a topology-group base",
                ));
            };
            if visual.kind != VisualKind::Model || visual.variant & !7 != 0 {
                return Err(invalid("stair visual has invalid kind or transform"));
            }
            referenced_stair_bases[base_index] = true;
        }
        if visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            && !visual.flags.contains(BlockFlags::CUBE_GEOMETRY)
            && (visual.kind != VisualKind::Model
                || visual.model_template == NO_MODEL_TEMPLATE
                || compiled
                    .model_templates
                    .get(visual.model_template as usize)
                    .is_none_or(|template| template.quad_count == 0))
        {
            return Err(invalid(
                "standalone full-face occlusion requires a drawable model",
            ));
        }
    }
    if referenced_stair_bases.iter().any(|referenced| !referenced) {
        return Err(invalid("stair template group is unreferenced"));
    }
    let mut previous = None;
    for &(hash, visual) in &compiled.hashed {
        if previous.is_some_and(|value| value >= hash) || visual as usize >= compiled.visuals.len()
        {
            return Err(invalid("hash table is not canonical"));
        }
        previous = Some(hash);
    }
    let mut expected_quad = 0usize;
    for template in &compiled.model_templates {
        if template.flags & !MODEL_TEMPLATE_FLAGS_MASK != 0
            || template.quad_start as usize != expected_quad
            || template.quad_count > 32
            || (template.flags & MODEL_TEMPLATE_FLAG_KELP != 0 && template.quad_count != 6)
        {
            return Err(invalid("model template spans are not canonical"));
        }
        expected_quad = add(
            expected_quad,
            template.quad_count as usize,
            "template quads",
        )?;
    }
    if expected_quad != compiled.model_quads.len() {
        return Err(invalid("model templates do not cover all quads"));
    }
    for template in &compiled.model_templates {
        if template.flags & MODEL_TEMPLATE_FLAG_KELP == 0 {
            continue;
        }
        let start = template.quad_start as usize;
        let quads = &compiled.model_quads[start..start + 6];
        if quads[..4]
            .iter()
            .any(|quad| quad.flags & MODEL_QUAD_FLAG_TWO_SIDED != 0)
            || quads[4..]
                .iter()
                .any(|quad| quad.flags & MODEL_QUAD_FLAG_TWO_SIDED == 0)
        {
            return Err(invalid("kelp template has noncanonical sidedness"));
        }
    }
    for quad in &compiled.model_quads {
        if quad.material as usize >= compiled.materials.len()
            || !model_quad_flags_are_valid(quad.flags)
        {
            return Err(invalid("model quad has invalid material or flags"));
        }
    }
    let mut expected_frame = 0usize;
    for animation in &compiled.animations {
        if animation.frame_start as usize != expected_frame
            || animation.frame_count == 0
            || animation.ticks_per_frame == 0
            || animation.replicate == 0
            || animation.flags & !ANIMATION_FLAGS_MASK != 0
        {
            return Err(invalid("animation spans are not canonical"));
        }
        expected_frame = add(
            expected_frame,
            animation.frame_count as usize,
            "animation frames",
        )?;
    }
    if expected_frame != compiled.animation_frames.len() {
        return Err(invalid("animations do not cover all frames"));
    }
    for &frame in &compiled.animation_frames {
        validate_texture_ref(frame, &compiled.texture_pages, "animation frame")?;
    }
    Ok(())
}

fn compiled_compound_tails(templates: &[crate::ModelTemplate]) -> Result<Vec<bool>, AssetError> {
    let mut tails = vec![false; templates.len()];
    for (index, template) in templates.iter().enumerate() {
        if template.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            continue;
        }
        if template.flags != MODEL_TEMPLATE_FLAG_COMPOUND_NEXT {
            return Err(invalid("compound template head has incompatible flags"));
        }
        if template.quad_count == 0 {
            return Err(invalid("compound template head has no quads"));
        }
        let Some(tail) = templates.get(index + 1) else {
            return Err(invalid("compound template pair is truncated"));
        };
        if tail.flags != 0 {
            return Err(invalid("compound continuation is not a plain template"));
        }
        if tail.quad_count == 0 {
            return Err(invalid("compound continuation has no quads"));
        }
        tails[index + 1] = true;
    }
    Ok(tails)
}

fn compiled_stair_bases(templates: &[crate::ModelTemplate]) -> Result<Vec<usize>, AssetError> {
    let mut bases = Vec::new();
    let mut index = 0;
    while index < templates.len() {
        if templates[index].flags & MODEL_TEMPLATE_FLAG_STAIR == 0 {
            index += 1;
            continue;
        }
        let Some(group) = templates.get(index..index + 5) else {
            return Err(invalid("stair template group is truncated"));
        };
        if group
            .iter()
            .any(|template| template.flags != MODEL_TEMPLATE_FLAG_STAIR || template.quad_count == 0)
        {
            return Err(invalid("stair template group is noncanonical"));
        }
        bases.push(index);
        index += 5;
    }
    Ok(bases)
}

fn validate_texture(index: usize, texture: &crate::TextureArray) -> Result<(), AssetError> {
    let layers = texture.layers as usize;
    if layers == 0 || layers > MAX_TEXTURE_LAYERS {
        return Err(invalid(format!(
            "texture page {index} has invalid layer count"
        )));
    }
    if texture.mips.len() != MIP_COUNT as usize {
        return Err(invalid(format!(
            "texture page {index} has invalid mip count"
        )));
    }
    for (level, mip) in texture.mips.iter().enumerate() {
        let side = TILE_SIZE >> level;
        if mip.size != side || mip.rgba8.len() != side as usize * side as usize * 4 * layers {
            return Err(invalid(format!(
                "texture page {index} mip {level} is malformed"
            )));
        }
    }
    Ok(())
}

fn validate_texture_ref(
    reference: TextureRef,
    pages: &[crate::TexturePage],
    context: &str,
) -> Result<(), AssetError> {
    let page = reference.page() as usize;
    if page >= pages.len() || reference.layer() >= pages[page].texture.layers {
        return Err(invalid(format!(
            "{context} has invalid texture reference {:#010x}",
            reference.raw()
        )));
    }
    Ok(())
}

fn optional_id(id: u32, len: usize, context: &str) -> Result<(), AssetError> {
    if id != u32::MAX && id as usize >= len {
        return Err(invalid(format!("{context} {id} is out of range")));
    }
    Ok(())
}

fn texture_digest(texture: &crate::TextureArray) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for mip in &texture.mips {
        hasher.update(&mip.rgba8);
    }
    hasher.finalize().into()
}

fn texture_length(texture: &crate::TextureArray) -> Result<usize, AssetError> {
    texture
        .mips
        .iter()
        .try_fold(0usize, |sum, mip| add(sum, mip.rgba8.len(), "texture page"))
}

fn bounded(section: &'static str, count: usize, max: usize) -> Result<(), AssetError> {
    if count > max {
        return Err(invalid(format!(
            "{section} count {count} exceeds limit {max}"
        )));
    }
    Ok(())
}

/// Writes a blob through a unique sibling temporary file and an atomic rename.
pub fn write_blob_atomic(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| AssetError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let file_name = path.file_name().ok_or_else(|| AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "output path has no file name"),
    })?;
    let mut temporary = None;
    for attempt in 0..64_u32 {
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            attempt
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: candidate,
                    source,
                });
            }
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve an atomic output file",
        ),
    })?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)
    })();
    if let Err(source) = result {
        let _ = fs::remove_file(&temporary_path);
        return Err(AssetError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn size(count: usize, width: usize, section: &'static str) -> Result<usize, AssetError> {
    count
        .checked_mul(width)
        .ok_or(AssetError::BlobSizeOverflow { section })
}
fn add(left: usize, right: usize, section: &'static str) -> Result<usize, AssetError> {
    left.checked_add(right)
        .ok_or(AssetError::BlobSizeOverflow { section })
}
fn count(value: usize, section: &'static str) -> Result<u32, AssetError> {
    u32::try_from(value).map_err(|_| AssetError::BlobSizeOverflow { section })
}
fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push_offset(bytes: &mut Vec<u8>, value: usize, section: &'static str) -> Result<(), AssetError> {
    bytes.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| AssetError::BlobSizeOverflow { section })?
            .to_le_bytes(),
    );
    Ok(())
}
fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

const _: () = assert!(DIAGNOSTIC_MATERIAL == 0);
