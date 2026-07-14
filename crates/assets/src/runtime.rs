use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::model::{MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAGS_MASK};
use crate::{
    ANIMATION_FLAG_BLEND, Animation, AssetError, BLOB_MAGIC, BLOB_VERSION, BiomeRule, BlockFace,
    BlockFlags, BlockVisual, CompiledBiomeAssets, ContributorRole, DIAGNOSTIC_MATERIAL,
    MAX_ANIMATION_FRAMES, MAX_ANIMATIONS, MAX_BIOME_NAME_BYTES, MAX_BIOME_NAMES_BYTES,
    MAX_BIOME_RULES, MAX_MATERIALS, MAX_MODEL_QUADS, MAX_MODEL_TEMPLATES, MAX_TEXTURE_LAYERS,
    MAX_TEXTURE_PAGES, MIP_COUNT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_STAIR, Material, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    TILE_SIZE, TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE, TextureArray, TextureMip,
    TexturePage, TextureRef, TintSource, VisualKind,
    biome::{BIOME_RULE_FLAGS_MASK, validate_biome_assets},
    blob::{
        ANIMATION_BYTES, BIOME_RULE_BYTES, FRAME_BYTES, HASH_BYTES, HASH_ENTRY_BYTES, HEADER_BYTES,
        MATERIAL_BYTES, MAX_VISUALS, PAGE_BYTES, QUAD_BYTES, TEMPLATE_BYTES, VISUAL_BYTES,
    },
    compiler::{material_flags_are_valid, visual_semantics_are_valid},
    model::model_quad_flags_are_valid,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkIdMode {
    Sequential,
    Hashed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedFace {
    material_id: u32,
}
impl ResolvedFace {
    #[must_use]
    pub const fn material_id(self) -> u32 {
        self.material_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedBlock {
    visual: BlockVisual,
    known: bool,
}
impl ResolvedBlock {
    const fn known(visual: BlockVisual) -> Self {
        Self {
            visual,
            known: true,
        }
    }
    const fn diagnostic() -> Self {
        Self {
            visual: diagnostic_visual(),
            known: false,
        }
    }
    #[must_use]
    pub const fn is_known(self) -> bool {
        self.known
    }
    #[must_use]
    pub const fn flags(self) -> BlockFlags {
        self.visual.flags
    }
    #[must_use]
    pub const fn face(self, face: BlockFace) -> ResolvedFace {
        ResolvedFace {
            material_id: self.visual.faces[face as usize],
        }
    }
    #[must_use]
    pub const fn kind(self) -> VisualKind {
        self.visual.kind
    }
    #[must_use]
    pub const fn contributor_role(self) -> ContributorRole {
        self.visual.contributor_role
    }
    #[must_use]
    pub const fn model_template(self) -> Option<u32> {
        if self.visual.model_template == NO_MODEL_TEMPLATE {
            None
        } else {
            Some(self.visual.model_template)
        }
    }
    #[must_use]
    pub const fn animation(self) -> Option<u32> {
        if self.visual.animation == NO_ANIMATION {
            None
        } else {
            Some(self.visual.animation)
        }
    }
    #[must_use]
    pub const fn variant(self) -> u32 {
        self.visual.variant
    }
}

const fn diagnostic_visual() -> BlockVisual {
    BlockVisual {
        faces: [DIAGNOSTIC_MATERIAL; 6],
        flags: BlockFlags::empty(),
        kind: VisualKind::Diagnostic,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    }
}

pub struct RuntimeAssets {
    visuals: Box<[BlockVisual]>,
    hashed: Box<[(u32, u32)]>,
    materials: Box<[Material]>,
    model_templates: Box<[ModelTemplate]>,
    model_quads: Box<[ModelQuad]>,
    animations: Box<[Animation]>,
    animation_frames: Box<[TextureRef]>,
    texture_pages: Box<[TexturePage]>,
    biomes: CompiledBiomeAssets,
    missing: AtomicU64,
}

impl RuntimeAssets {
    #[must_use]
    pub fn diagnostic() -> Self {
        let mips = [16_u32, 8, 4, 2, 1]
            .into_iter()
            .map(|size| {
                let mut rgba8 = Vec::with_capacity(size as usize * size as usize * 4);
                for y in 0..size {
                    for x in 0..size {
                        rgba8.extend_from_slice(if (x + y) & 1 == 0 {
                            &[255, 0, 255, 255]
                        } else {
                            &[0, 0, 0, 255]
                        });
                    }
                }
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            visuals: vec![diagnostic_visual()].into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
            }]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })]
                .into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
            missing: AtomicU64::new(0),
        }
    }

    /// Validates the complete `MCBEAS04` envelope and every cross-reference before allocating tables.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        let header = Header::decode(bytes)?;
        header.validate_layout(bytes)?;
        let sections = header.sections(bytes);
        validate_hash(bytes, header.offsets[12])?;
        let page_meta = validate_pages(
            sections[7],
            sections[8],
            header.page_count,
            header.offsets[8],
        )?;
        validate_fixed(&header, &sections, &page_meta)?;
        let biomes = decode_biomes(sections[9], sections[10], sections[11])?;
        Ok(Self {
            visuals: decode_visuals(sections[0])?,
            hashed: decode_hashes(sections[1]),
            materials: decode_materials(sections[2])?,
            model_templates: decode_templates(sections[3]),
            model_quads: decode_quads(sections[4]),
            animations: decode_animations(sections[5]),
            animation_frames: decode_frames(sections[6])?,
            texture_pages: decode_pages(sections[8], &page_meta)?,
            biomes,
            missing: AtomicU64::new(0),
        })
    }

    #[must_use]
    pub fn resolve(&self, mode: NetworkIdMode, value: u32) -> ResolvedBlock {
        let visual = match mode {
            NetworkIdMode::Sequential => self.visuals.get(value as usize).copied(),
            NetworkIdMode::Hashed => self
                .sequential_id_for_hash(value)
                .and_then(|sequential_id| self.visuals.get(sequential_id as usize))
                .copied(),
        };
        visual.map_or_else(
            || {
                self.record_missing();
                ResolvedBlock::diagnostic()
            },
            ResolvedBlock::known,
        )
    }

    /// Returns the exact sequential identity paired with a validated network
    /// hash. Coverage tooling uses this rather than visual equality because
    /// distinct states may intentionally share byte-identical visuals.
    #[must_use]
    pub fn sequential_id_for_hash(&self, network_hash: u32) -> Option<u32> {
        self.hashed
            .binary_search_by_key(&network_hash, |entry| entry.0)
            .ok()
            .map(|index| self.hashed[index].1)
    }

    /// Number of sequential visual records in the validated runtime blob.
    #[must_use]
    pub const fn visual_count(&self) -> usize {
        self.visuals.len()
    }

    /// Number of unique network-hash mappings in the validated runtime blob.
    #[must_use]
    pub const fn hashed_count(&self) -> usize {
        self.hashed.len()
    }

    #[must_use]
    pub fn material(&self, id: u32) -> Material {
        self.materials.get(id as usize).copied().unwrap_or_else(|| {
            self.record_missing();
            self.materials[0]
        })
    }
    #[must_use]
    pub const fn materials(&self) -> &[Material] {
        &self.materials
    }
    #[must_use]
    pub const fn model_templates(&self) -> &[ModelTemplate] {
        &self.model_templates
    }
    #[must_use]
    pub const fn model_quads(&self) -> &[ModelQuad] {
        &self.model_quads
    }
    #[must_use]
    pub const fn animations(&self) -> &[Animation] {
        &self.animations
    }
    #[must_use]
    pub const fn animation_frames(&self) -> &[TextureRef] {
        &self.animation_frames
    }
    #[must_use]
    pub const fn texture_pages(&self) -> &[TexturePage] {
        &self.texture_pages
    }
    #[must_use]
    pub const fn texture_array(&self) -> &TextureArray {
        &self.texture_pages[0].texture
    }
    #[must_use]
    pub const fn biome_assets(&self) -> &CompiledBiomeAssets {
        &self.biomes
    }
    #[must_use]
    pub fn missing_count(&self) -> u64 {
        self.missing.load(Ordering::Relaxed)
    }
    fn record_missing(&self) {
        self.missing.fetch_add(1, Ordering::Relaxed);
    }
}

struct Header {
    counts: [usize; 8],
    page_count: usize,
    biome_count: usize,
    offsets: [usize; 13],
}
impl Header {
    fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEAS04 blob"));
        }
        if bytes[..8] != BLOB_MAGIC {
            return Err(invalid("invalid MCBEAS04 magic"));
        }
        if u32_at(bytes, 8) != BLOB_VERSION
            || u32_at(bytes, 12) != TILE_SIZE
            || u32_at(bytes, 16) != MIP_COUNT
        {
            return Err(invalid("unsupported MCBEAS04 header"));
        }
        if bytes[64..96] != [0; 32] {
            return Err(invalid("header reserved bytes are non-zero"));
        }
        if u32_at(bytes, 52) != TINT_MAP_COUNT as u32 || u32_at(bytes, 56) != TINT_MAP_SIZE {
            return Err(invalid("invalid tint-map dimensions"));
        }
        let counts = [20, 24, 28, 32, 36, 40, 44, 48].map(|offset| u32_at(bytes, offset) as usize);
        let mut offsets = [0usize; 13];
        for (index, value) in offsets.iter_mut().enumerate() {
            *value = usize::try_from(u64_at(bytes, 96 + index * 8))
                .map_err(|_| invalid("section offset exceeds platform"))?;
        }
        Ok(Self {
            counts,
            page_count: counts[7],
            biome_count: u32_at(bytes, 60) as usize,
            offsets,
        })
    }
    fn validate_layout(&self, bytes: &[u8]) -> Result<(), AssetError> {
        for (name, value, max) in [
            ("visual", self.counts[0], MAX_VISUALS),
            ("hash", self.counts[1], MAX_VISUALS),
            ("material", self.counts[2], MAX_MATERIALS),
            ("template", self.counts[3], MAX_MODEL_TEMPLATES),
            ("quad", self.counts[4], MAX_MODEL_QUADS),
            ("animation", self.counts[5], MAX_ANIMATIONS),
            ("frame", self.counts[6], MAX_ANIMATION_FRAMES),
            ("page", self.page_count, MAX_TEXTURE_PAGES),
            ("biome", self.biome_count, MAX_BIOME_RULES),
        ] {
            if value > max {
                return Err(invalid(format!("{name} count exceeds limit")));
            }
        }
        if self.page_count == 0 {
            return Err(invalid("blob has no texture page"));
        }
        let widths = [
            VISUAL_BYTES,
            HASH_ENTRY_BYTES,
            MATERIAL_BYTES,
            TEMPLATE_BYTES,
            QUAD_BYTES,
            ANIMATION_BYTES,
            FRAME_BYTES,
            PAGE_BYTES,
        ];
        let mut expected = HEADER_BYTES;
        for (index, width) in widths.into_iter().enumerate() {
            if self.offsets[index] != expected {
                return Err(invalid(
                    "blob sections overlap, have gaps, or are noncanonical",
                ));
            }
            expected = checked_add(
                expected,
                checked_mul(self.counts[index], width, "section")?,
                "section",
            )?;
        }
        if self.offsets[8] != expected {
            return Err(invalid("texture payload offset is noncanonical"));
        }
        if self.offsets[9] < self.offsets[8]
            || self.offsets[10] != checked_add(self.offsets[9], TINT_MAP_BYTES, "tint maps")?
            || self.offsets[11]
                != checked_add(
                    self.offsets[10],
                    checked_mul(self.biome_count, BIOME_RULE_BYTES, "biome rules")?,
                    "biome rules",
                )?
        {
            return Err(invalid("biome sections are noncanonical"));
        }
        if self.offsets[12] < self.offsets[11]
            || self.offsets[12] - self.offsets[11] > MAX_BIOME_NAMES_BYTES
            || bytes.len() != checked_add(self.offsets[12], HASH_BYTES, "blob hash")?
        {
            return Err(invalid("payload length is invalid"));
        }
        Ok(())
    }
    fn sections<'a>(&self, bytes: &'a [u8]) -> [&'a [u8]; 12] {
        std::array::from_fn(|index| &bytes[self.offsets[index]..self.offsets[index + 1]])
    }
}

#[derive(Clone, Copy)]
struct PageMeta {
    layers: usize,
    relative_offset: usize,
    length: usize,
}
fn validate_pages(
    records: &[u8],
    payload: &[u8],
    count: usize,
    payload_offset: usize,
) -> Result<Box<[PageMeta]>, AssetError> {
    let mut metas = Vec::with_capacity(count);
    let mut expected = payload_offset;
    for (index, record) in records.chunks_exact(PAGE_BYTES).enumerate() {
        if u32_at(record, 0) as usize != index
            || u32_at(record, 8) != MIP_COUNT
            || u32_at(record, 12) != 0
        {
            return Err(invalid("texture page descriptor is noncanonical"));
        }
        let layers = u32_at(record, 4) as usize;
        if layers == 0 || layers > MAX_TEXTURE_LAYERS {
            return Err(invalid("texture page layer count is invalid"));
        }
        let absolute = usize::try_from(u64_at(record, 16))
            .map_err(|_| invalid("page offset exceeds platform"))?;
        let length = usize::try_from(u64_at(record, 24))
            .map_err(|_| invalid("page length exceeds platform"))?;
        let expected_length = texture_byte_length(layers)?;
        if length != expected_length {
            return Err(invalid("texture page payload length is invalid"));
        }
        if absolute != expected {
            return Err(invalid(
                "texture page payloads overlap, have gaps, or are unordered",
            ));
        }
        expected = checked_add(expected, length, "page payload")?;
        let relative_offset = metas.iter().try_fold(0usize, |total, meta: &PageMeta| {
            checked_add(total, meta.length, "page relative offset")
        })?;
        let relative_end = checked_add(relative_offset, length, "page relative end")?;
        let data = payload
            .get(relative_offset..relative_end)
            .ok_or_else(|| invalid("texture page exceeds payload section"))?;
        if Sha256::digest(data).as_slice() != &record[32..64] {
            return Err(invalid("texture page SHA-256 mismatch"));
        }
        metas.push(PageMeta {
            layers,
            relative_offset,
            length,
        });
    }
    let covered = metas.iter().try_fold(0usize, |total, meta| {
        checked_add(total, meta.length, "page payload coverage")
    })?;
    if covered != payload.len() {
        return Err(invalid("texture page descriptors do not cover payload"));
    }
    Ok(metas.into_boxed_slice())
}

fn validate_fixed(
    header: &Header,
    sections: &[&[u8]; 12],
    pages: &[PageMeta],
) -> Result<(), AssetError> {
    let compound_tails = runtime_compound_tails(sections[3])?;
    let stair_bases = runtime_stair_bases(sections[3])?;
    let mut referenced_stair_bases = vec![false; stair_bases.len()];
    for (index, record) in sections[0].chunks_exact(VISUAL_BYTES).enumerate() {
        for face in 0..6 {
            if u32_at(record, face * 4) as usize >= header.counts[2] {
                return Err(invalid(format!("visual {index} has invalid material")));
            }
        }
        let flags =
            BlockFlags::from_bits(record[24]).ok_or_else(|| invalid("visual has unknown flags"))?;
        if !flags.has_valid_semantics() || record[27] != 0 {
            return Err(invalid("visual flags/reserved bytes are invalid"));
        }
        let kind = VisualKind::from_raw(record[25])?;
        let contributor_role = ContributorRole::read(record[26])?;
        if !visual_semantics_are_valid(kind, flags, contributor_role) {
            return Err(invalid("visual kind, flags, and contributor role disagree"));
        }
        let template = u32_at(record, 28);
        let animation = u32_at(record, 32);
        valid_optional(template, header.counts[3], "visual template")?;
        if template != NO_MODEL_TEMPLATE && compound_tails[template as usize] {
            return Err(invalid(
                "compound continuation cannot be directly visual-referenced",
            ));
        }
        valid_optional(animation, header.counts[5], "visual animation")?;
        if (matches!(kind, VisualKind::Model | VisualKind::Cross))
            != (template != NO_MODEL_TEMPLATE)
        {
            return Err(invalid("visual kind/template disagree"));
        }
        if template != NO_MODEL_TEMPLATE
            && u32_at(
                sections[3]
                    .chunks_exact(TEMPLATE_BYTES)
                    .nth(template as usize)
                    .expect("validated template reference"),
                8,
            ) & MODEL_TEMPLATE_FLAG_STAIR
                != 0
        {
            let Some(base_index) = stair_bases
                .iter()
                .position(|&base| base == template as usize)
            else {
                return Err(invalid(
                    "stair visual does not reference a topology-group base",
                ));
            };
            if kind != VisualKind::Model || u32_at(record, 36) & !7 != 0 {
                return Err(invalid("stair visual has invalid kind or transform"));
            }
            referenced_stair_bases[base_index] = true;
        }
        if flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            && !flags.contains(BlockFlags::CUBE_GEOMETRY)
            && (kind != VisualKind::Model
                || template == NO_MODEL_TEMPLATE
                || sections[3]
                    .chunks_exact(TEMPLATE_BYTES)
                    .nth(template as usize)
                    .is_none_or(|template| u32_at(template, 4) == 0))
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
    for record in sections[1].chunks_exact(8) {
        let hash = u32_at(record, 0);
        if previous.is_some_and(|p| p >= hash) || u32_at(record, 4) as usize >= header.counts[0] {
            return Err(invalid("hash lookup is noncanonical"));
        }
        previous = Some(hash);
    }
    for (index, record) in sections[2].chunks_exact(MATERIAL_BYTES).enumerate() {
        let texture = TextureRef::from_raw(u32_at(record, 0))?;
        valid_ref(texture, pages)?;
        let flags = u32_at(record, 4);
        if !material_flags_are_valid(flags) {
            return Err(invalid("material flags are invalid"));
        }
        valid_optional(u32_at(record, 8), header.counts[5], "material animation")?;
        if index == 0
            && (texture != TextureRef::DIAGNOSTIC
                || flags != 0
                || u32_at(record, 8) != NO_ANIMATION)
        {
            return Err(invalid("material zero is not diagnostic"));
        }
    }
    let mut quad = 0usize;
    for record in sections[3].chunks_exact(TEMPLATE_BYTES) {
        if u32_at(record, 0) as usize != quad
            || u32_at(record, 4) > 32
            || u32_at(record, 8) & !MODEL_TEMPLATE_FLAGS_MASK != 0
            || (u32_at(record, 8) & MODEL_TEMPLATE_FLAG_KELP != 0 && u32_at(record, 4) != 6)
        {
            return Err(invalid("template spans are noncanonical"));
        }
        quad = checked_add(quad, u32_at(record, 4) as usize, "template")?;
    }
    if quad != header.counts[4] {
        return Err(invalid("templates do not cover quads"));
    }
    for record in sections[3].chunks_exact(TEMPLATE_BYTES) {
        if u32_at(record, 8) & MODEL_TEMPLATE_FLAG_KELP == 0 {
            continue;
        }
        let start = u32_at(record, 0) as usize;
        let mut quads = sections[4].chunks_exact(QUAD_BYTES).skip(start).take(6);
        let body_is_one_sided = quads
            .by_ref()
            .take(4)
            .all(|quad| u32_at(quad, 44) & MODEL_QUAD_FLAG_TWO_SIDED == 0);
        let head_is_two_sided = quads.all(|quad| u32_at(quad, 44) & MODEL_QUAD_FLAG_TWO_SIDED != 0);
        if !body_is_one_sided || !head_is_two_sided {
            return Err(invalid("kelp template has noncanonical sidedness"));
        }
    }
    for record in sections[4].chunks_exact(QUAD_BYTES) {
        if u32_at(record, 40) as usize >= header.counts[2]
            || !model_quad_flags_are_valid(u32_at(record, 44))
        {
            return Err(invalid("model quad is invalid"));
        }
    }
    let mut frame = 0usize;
    for record in sections[5].chunks_exact(ANIMATION_BYTES) {
        if u32_at(record, 0) as usize != frame
            || u32_at(record, 4) == 0
            || u32_at(record, 8) == 0
            || u32_at(record, 20) == 0
            || u32_at(record, 24) & !ANIMATION_FLAG_BLEND != 0
        {
            return Err(invalid("animation is noncanonical"));
        }
        frame = checked_add(frame, u32_at(record, 4) as usize, "animation")?;
    }
    if frame != header.counts[6] {
        return Err(invalid("animations do not cover frames"));
    }
    for record in sections[6].chunks_exact(FRAME_BYTES) {
        valid_ref(TextureRef::from_raw(u32_at(record, 0))?, pages)?;
    }
    validate_biome_sections(sections[9], sections[10], sections[11], header.biome_count)
}

fn runtime_stair_bases(bytes: &[u8]) -> Result<Vec<usize>, AssetError> {
    let records = bytes.chunks_exact(TEMPLATE_BYTES).collect::<Vec<_>>();
    let mut bases = Vec::new();
    let mut index = 0;
    while index < records.len() {
        if u32_at(records[index], 8) & MODEL_TEMPLATE_FLAG_STAIR == 0 {
            index += 1;
            continue;
        }
        let Some(group) = records.get(index..index + 5) else {
            return Err(invalid("stair template group is truncated"));
        };
        if group
            .iter()
            .any(|record| u32_at(record, 8) != MODEL_TEMPLATE_FLAG_STAIR || u32_at(record, 4) == 0)
        {
            return Err(invalid("stair template group is noncanonical"));
        }
        bases.push(index);
        index += 5;
    }
    Ok(bases)
}

fn runtime_compound_tails(bytes: &[u8]) -> Result<Vec<bool>, AssetError> {
    let records = bytes.chunks_exact(TEMPLATE_BYTES).collect::<Vec<_>>();
    let mut tails = vec![false; records.len()];
    for (index, record) in records.iter().enumerate() {
        let flags = u32_at(record, 8);
        if flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            continue;
        }
        if flags != MODEL_TEMPLATE_FLAG_COMPOUND_NEXT {
            return Err(invalid("compound template head has incompatible flags"));
        }
        if u32_at(record, 4) == 0 {
            return Err(invalid("compound template head has no quads"));
        }
        let Some(tail) = records.get(index + 1) else {
            return Err(invalid("compound template pair is truncated"));
        };
        if u32_at(tail, 8) != 0 {
            return Err(invalid("compound continuation is not a plain template"));
        }
        if u32_at(tail, 4) == 0 {
            return Err(invalid("compound continuation has no quads"));
        }
        tails[index + 1] = true;
    }
    Ok(tails)
}

fn valid_ref(reference: TextureRef, pages: &[PageMeta]) -> Result<(), AssetError> {
    let page = reference.page() as usize;
    if page >= pages.len() || reference.layer() as usize >= pages[page].layers {
        return Err(invalid("bad page/layer texture reference"));
    }
    Ok(())
}
fn valid_optional(id: u32, len: usize, what: &str) -> Result<(), AssetError> {
    if id != u32::MAX && id as usize >= len {
        return Err(invalid(format!("{what} is out of range")));
    }
    Ok(())
}

fn decode_visuals(bytes: &[u8]) -> Result<Box<[BlockVisual]>, AssetError> {
    bytes
        .chunks_exact(VISUAL_BYTES)
        .map(|r| {
            let mut faces = [0; 6];
            for (i, v) in faces.iter_mut().enumerate() {
                *v = u32_at(r, i * 4);
            }
            Ok(BlockVisual {
                faces,
                flags: BlockFlags::from_bits(r[24]).expect("validated"),
                kind: VisualKind::from_raw(r[25])?,
                contributor_role: ContributorRole::read(r[26])?,
                model_template: u32_at(r, 28),
                animation: u32_at(r, 32),
                variant: u32_at(r, 36),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}
fn decode_hashes(bytes: &[u8]) -> Box<[(u32, u32)]> {
    bytes
        .chunks_exact(8)
        .map(|r| (u32_at(r, 0), u32_at(r, 4)))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
fn decode_materials(bytes: &[u8]) -> Result<Box<[Material]>, AssetError> {
    bytes
        .chunks_exact(12)
        .map(|r| {
            Ok(Material {
                texture: TextureRef::from_raw(u32_at(r, 0))?,
                flags: u32_at(r, 4),
                animation: u32_at(r, 8),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}
fn decode_templates(bytes: &[u8]) -> Box<[ModelTemplate]> {
    bytes
        .chunks_exact(12)
        .map(|r| ModelTemplate {
            quad_start: u32_at(r, 0),
            quad_count: u32_at(r, 4),
            flags: u32_at(r, 8),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
fn decode_quads(bytes: &[u8]) -> Box<[ModelQuad]> {
    bytes
        .chunks_exact(QUAD_BYTES)
        .map(|r| {
            let mut positions = [[0; 3]; 4];
            for (v, p) in positions.iter_mut().flatten().enumerate() {
                *p = i16::from_le_bytes(r[v * 2..v * 2 + 2].try_into().unwrap());
            }
            let mut uvs = [[0; 2]; 4];
            for (v, p) in uvs.iter_mut().flatten().enumerate() {
                *p = u16::from_le_bytes(r[24 + v * 2..26 + v * 2].try_into().unwrap());
            }
            ModelQuad {
                positions,
                uvs,
                material: u32_at(r, 40),
                flags: u32_at(r, 44),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
fn decode_animations(bytes: &[u8]) -> Box<[Animation]> {
    bytes
        .chunks_exact(ANIMATION_BYTES)
        .map(|r| Animation {
            frame_start: u32_at(r, 0),
            frame_count: u32_at(r, 4),
            ticks_per_frame: u32_at(r, 8),
            atlas_index: u32_at(r, 12),
            atlas_tile_variant: u32_at(r, 16),
            replicate: u32_at(r, 20),
            flags: u32_at(r, 24),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
fn decode_frames(bytes: &[u8]) -> Result<Box<[TextureRef]>, AssetError> {
    bytes
        .chunks_exact(4)
        .map(|r| TextureRef::from_raw(u32_at(r, 0)))
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}
fn decode_pages(payload: &[u8], metas: &[PageMeta]) -> Result<Box<[TexturePage]>, AssetError> {
    metas
        .iter()
        .map(|m| {
            let data = &payload[m.relative_offset..m.relative_offset + m.length];
            let mut cursor = 0;
            let mut mips = Vec::with_capacity(MIP_COUNT as usize);
            for level in 0..MIP_COUNT {
                let side = TILE_SIZE >> level;
                let length = side as usize * side as usize * 4 * m.layers;
                mips.push(TextureMip {
                    size: side,
                    rgba8: data[cursor..cursor + length].to_vec().into_boxed_slice(),
                });
                cursor += length;
            }
            Ok(TexturePage::new(TextureArray {
                layers: m.layers as u32,
                mips: mips.into_boxed_slice(),
            }))
        })
        .collect::<Result<Vec<_>, AssetError>>()
        .map(Vec::into_boxed_slice)
}

fn validate_biome_sections(
    tints: &[u8],
    rules: &[u8],
    names: &[u8],
    count: usize,
) -> Result<(), AssetError> {
    if tints.len() != TINT_MAP_BYTES || rules.len() != count * BIOME_RULE_BYTES {
        return Err(invalid("biome section length mismatch"));
    }
    let mut previous = None;
    let mut expected = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for r in rules.chunks_exact(BIOME_RULE_BYTES) {
        let id = u32_at(r, 0);
        let offset = u32_at(r, 4) as usize;
        let length = u16::from_le_bytes(r[8..10].try_into().unwrap()) as usize;
        if previous.is_some_and(|p| p >= id)
            || offset != expected
            || length == 0
            || length > MAX_BIOME_NAME_BYTES
        {
            return Err(invalid("biome rules are noncanonical"));
        }
        let end = checked_add(offset, length, "biome name")?;
        let name = std::str::from_utf8(
            names
                .get(offset..end)
                .ok_or_else(|| invalid("biome name out of range"))?,
        )
        .map_err(|_| invalid("biome name is not UTF-8"))?;
        if !seen.insert(name)
            || u16::from_le_bytes(r[10..12].try_into().unwrap()) & !BIOME_RULE_FLAGS_MASK != 0
        {
            return Err(invalid("biome rule is invalid"));
        }
        for offset in [12, 16, 20, 24] {
            TintSource::from_raw(u32_at(r, offset))?;
        }
        if u32_at(r, 24) >> 24 != 0
            || !f32::from_bits(u32_at(r, 28)).is_finite()
            || !f32::from_bits(u32_at(r, 32)).is_finite()
        {
            return Err(invalid("biome values are invalid"));
        }
        expected = end;
        previous = Some(id);
    }
    if expected != names.len() {
        return Err(invalid("biome names have trailing bytes"));
    }
    Ok(())
}
fn decode_biomes(
    tints: &[u8],
    rules: &[u8],
    names: &[u8],
) -> Result<CompiledBiomeAssets, AssetError> {
    let rules = rules
        .chunks_exact(36)
        .map(|r| {
            let o = u32_at(r, 4) as usize;
            let l = u16::from_le_bytes(r[8..10].try_into().unwrap()) as usize;
            Ok(BiomeRule {
                id: u32_at(r, 0),
                name: std::str::from_utf8(&names[o..o + l])
                    .expect("validated")
                    .into(),
                flags: u16::from_le_bytes(r[10..12].try_into().unwrap()),
                grass: TintSource::from_raw(u32_at(r, 12))?,
                foliage: TintSource::from_raw(u32_at(r, 16))?,
                dry_foliage: TintSource::from_raw(u32_at(r, 20))?,
                water: TintSource::from_raw(u32_at(r, 24))?,
                temperature_bits: u32_at(r, 28),
                downfall_bits: u32_at(r, 32),
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    let result = CompiledBiomeAssets {
        tint_maps_rgb8: tints.to_vec().into_boxed_slice(),
        rules: rules.into_boxed_slice(),
    };
    validate_biome_assets(&result)?;
    Ok(result)
}

fn validate_hash(bytes: &[u8], payload: usize) -> Result<(), AssetError> {
    if Sha256::digest(&bytes[..payload]).as_slice() != &bytes[payload..] {
        return Err(invalid("compiled asset SHA-256 mismatch"));
    }
    Ok(())
}
fn texture_byte_length(layers: usize) -> Result<usize, AssetError> {
    let mut total = 0;
    for level in 0..MIP_COUNT {
        let side = (TILE_SIZE >> level) as usize;
        total = checked_add(
            total,
            checked_mul(checked_mul(side, side, "mip")?, 4 * layers, "mip")?,
            "texture",
        )?;
    }
    Ok(total)
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn checked_add(a: usize, b: usize, s: &'static str) -> Result<usize, AssetError> {
    a.checked_add(b)
        .ok_or(AssetError::BlobSizeOverflow { section: s })
}
fn checked_mul(a: usize, b: usize, s: &'static str) -> Result<usize, AssetError> {
    a.checked_mul(b)
        .ok_or(AssetError::BlobSizeOverflow { section: s })
}
fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
