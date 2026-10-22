//! Beacon base, core, and glass shell with independent pack textures.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::facts::pack::PackBlock};

/// Admits the built-in beacon's primary block state.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == PackBlock::Beacon.identifier()
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Resolves one selected block face without inheriting the cube fallback's material policy.
fn face_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
    flags: u32,
) -> Option<(Descriptor, Box<str>)> {
    let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    let (path, state_variant) = pack.terrain.get_for_record_variant(&key, record)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: flags
                | if rotate_uv {
                    MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                },
        },
        key,
    ))
}

/// Selects the bottom texture for the base, top for the core, and north for the shell.
/// A single shared sprite preserves the core and takes the base and shell from obsidian and glass.
pub(in crate::compiler) fn descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<[(Descriptor, Box<str>); 3]> {
    let mut base = face_descriptor(pack, record, BlockFace::Down, 0)?;
    let core = face_descriptor(pack, record, BlockFace::Up, MATERIAL_FLAG_ALPHA_CUTOUT)?;
    let mut shell = face_descriptor(pack, record, BlockFace::North, MATERIAL_FLAG_ALPHA_CUTOUT)?;
    let same_sprite = |left: &Descriptor, right: &Descriptor| {
        left.path == right.path && left.state_variant == right.state_variant
    };
    if same_sprite(&base.0, &core.0) && same_sprite(&shell.0, &core.0) {
        let mut companion = record.clone();
        companion.name = PackBlock::Obsidian.identifier().into();
        base = face_descriptor(pack, &companion, BlockFace::Up, 0)?;
        companion.name = PackBlock::Glass.identifier().into();
        shell = face_descriptor(pack, &companion, BlockFace::Up, MATERIAL_FLAG_ALPHA_CUTOUT)?;
    }
    Some([base, core, shell])
}

/// Compiles the three shells before the ordinary cube and collision fallback routes.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some(descriptors) = descriptors(inputs.pack, record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let [Some(base), Some(core), Some(shell)] =
        descriptors.map(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let materials = [base, core, shell];
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        // The base's 0.1-pixel bottom offset rounds to the nearest model coordinate.
        let parts = [
            (base, [32, 2, 32], [224, 48, 224]),
            (core, [48, 48, 48], [208, 224, 208]),
            (shell, [0, 0, 0], [256, 256, 256]),
        ];
        let quads = parts
            .into_iter()
            .flat_map(|(material, min, max)| {
                super::geometry::vanilla_cuboid_quads([material; 6], min, max)
            })
            .collect();
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(materials, template);
        template
    };
    set_model_visual(&mut visual, [shell; 6], template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}
