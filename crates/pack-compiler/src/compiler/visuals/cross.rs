use super::context::{ModelStorage, RuleInputs, diagnostic_visual, push_model_template};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// Fixed-point inset of an ordinary nine-tenths-width plant sprite.
pub(in crate::compiler) const PLANT_INSET: i16 = 13;
/// Fixed-point inset of narrow plant sprites.
pub(in crate::compiler) const NARROW_PLANT_INSET: i16 = 36;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum Geometry {
    Diagonal,
    CropRows,
    Torchflower,
    BerryBush,
    Reeds,
    Grass,
    ComponentCross { single_sided: bool },
}

pub(in crate::compiler) type TemplateKey = ([u32; 2], Geometry);

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_cross_visual(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    if block_render_fact(&record.name).kind == R::BerryBush
        && canonical_state_u32(&record.canonical_state, "growth")
            .filter(|&growth| growth <= 7)
            .is_none()
    {
        return Ok(CompileRuleResult::Reject);
    }
    let mut visual = diagnostic_visual(record);
    let faces = if is_aquatic_cross(record) {
        aquatic_cross_faces(record)
    } else {
        Some([cross_texture_face(record); 2])
    };
    if let Some(faces) = faces
        && let (Some(material_a), Some(material_b), Some(variant)) = (
            inputs.material(record, faces[0]),
            inputs.material(record, faces[1]),
            model_variant(inputs.pack, record, faces[0]),
        )
    {
        let materials = [material_a, material_b];
        let geometry = if let Some(material) = inputs.pack.blocks.component_cross(&record.name) {
            Geometry::ComponentCross {
                single_sided: material.single_sided,
            }
        } else {
            match block_render_fact(&record.name).kind {
                R::CropRows => Geometry::CropRows,
                R::Torchflower => Geometry::Torchflower,
                R::BerryBush => Geometry::BerryBush,
                R::GrassCross => Geometry::Grass,
                _ if shape_traits(record).contains(assets::facts::shape::ShapeTraits::REEDS) => {
                    Geometry::Reeds
                }
                _ => Geometry::Diagonal,
            }
        };
        let key = (materials, geometry);
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let quads = match geometry {
                Geometry::CropRows => crop_row_quads(material_a).to_vec(),
                Geometry::Torchflower => plant_quads(material_a, 256, 0, PLANT_INSET).to_vec(),
                Geometry::BerryBush => plant_quads(material_a, 256, 0, NARROW_PLANT_INSET).to_vec(),
                Geometry::Reeds => plant_quads(material_a, 256, 0, NARROW_PLANT_INSET).to_vec(),
                Geometry::Grass => plant_quads(material_a, 256, 0, NARROW_PLANT_INSET).to_vec(),
                Geometry::Diagonal => crossed_quads(materials).to_vec(),
                Geometry::ComponentCross { single_sided } => {
                    component_cross_quads(material_a, single_sided)
                }
            };
            let flags = if geometry == Geometry::Grass {
                assets::MODEL_TEMPLATE_FLAG_SHORT_GRASS
            } else {
                0
            };
            let template = push_model_template(quads, flags, storage.templates, storage.quads)?;
            templates.insert(key, template);
            template
        };
        visual.faces = [material_a; 6];
        visual.kind = VisualKind::Cross;
        visual.model_template = template;
        visual.variant = variant;
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Reads plant membership from the same shape rules that compile selection bounds.
fn shape_traits(record: &RegistryRecord) -> assets::facts::shape::ShapeTraits {
    use assets::facts::{rules, shape::ShapeTraits};
    let Ok(state) = serde_json::from_str(&record.canonical_state) else {
        return ShapeTraits::empty();
    };
    rules::evaluate(
        &record.name,
        &state,
        record.model_family,
        record.flags,
        &rules::policy().block_rules,
    )
    .2
}

/// The built-in cross has inset planes and independently textured front and back faces.
fn component_cross_quads(material: u32, single_sided: bool) -> Vec<ModelQuad> {
    let planes = plant_quads(material, 256, 0, PLANT_INSET);
    if !single_sided {
        return planes.to_vec();
    }
    planes
        .into_iter()
        .flat_map(|mut front| {
            front.flags = 0;
            let mut back = front;
            back.positions.swap(0, 1);
            back.positions.swap(2, 3);
            // Each authored face reads the whole image from its own outside view.
            back.uvs = front.uvs;
            [front, back]
        })
        .collect()
}

/// Emits four full-width crop rows, lowered 1/16 to meet farmland's surface.
fn crop_row_quads(material: u32) -> [ModelQuad; 4] {
    let uvs = [[0, 0], [0, 4096], [4096, 4096], [4096, 0]];
    [
        [[64, 240, 0], [64, -16, 0], [64, -16, 256], [64, 240, 256]],
        [
            [192, 240, 256],
            [192, -16, 256],
            [192, -16, 0],
            [192, 240, 0],
        ],
        [[0, 240, 64], [0, -16, 64], [256, -16, 64], [256, 240, 64]],
        [
            [256, 240, 192],
            [256, -16, 192],
            [0, -16, 192],
            [0, 240, 192],
        ],
    ]
    .map(|positions| ModelQuad {
        positions,
        uvs,
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    })
}

pub(in crate::compiler) fn model_variant(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<u32> {
    let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    pack.terrain
        .get_for_model_record(&key, record)
        .map(|(_, variant)| variant)
}

pub(in crate::compiler) fn crossed_quads(materials: [u32; 2]) -> [ModelQuad; 2] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    [
        ModelQuad {
            positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
            uvs,
            material: materials[0],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
            uvs,
            material: materials[1],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}

/// Emits an inset sprite cross with upright UVs and a caller-owned height and base.
pub(in crate::compiler) fn plant_quads(
    material: u32,
    height: i16,
    base: i16,
    inset: i16,
) -> [ModelQuad; 2] {
    let mut quads = crossed_quads([material; 2]);
    for quad in &mut quads {
        for (position, uv) in quad.positions.iter_mut().zip(&mut quad.uvs) {
            position[0] = if position[0] == 0 { inset } else { 256 - inset };
            position[2] = if position[2] == 0 { inset } else { 256 - inset };
            let bottom = position[1] == 0;
            position[1] = base + if bottom { 0 } else { height };
            uv[1] = if bottom { height as u16 * 16 } else { 0 };
        }
    }
    quads
}
