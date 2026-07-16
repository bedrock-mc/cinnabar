use super::super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct CuboidTemplateKey {
    pub(in crate::compiler) materials: [u32; 6],
    pub(in crate::compiler) min: [i16; 3],
    pub(in crate::compiler) max: [i16; 3],
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct PressurePlateTemplateKey {
    pub(in crate::compiler) materials: [u32; 6],
    pub(in crate::compiler) pressed: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct ButtonTemplateKey {
    pub(in crate::compiler) materials: [u32; 6],
    pub(in crate::compiler) orientation: u8,
    pub(in crate::compiler) pressed: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub(in crate::compiler) enum PaleMossCarpetSide {
    None = 0,
    Short = 1,
    Tall = 2,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct PaleMossCarpetState {
    pub(in crate::compiler) sides: [PaleMossCarpetSide; 4],
    pub(in crate::compiler) upper: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum CarpetState {
    Ordinary,
    Pale(PaleMossCarpetState),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct PaleMossCarpetTemplateKey {
    pub(in crate::compiler) materials: [u32; 6],
    pub(in crate::compiler) side_materials: [u32; 2],
    pub(in crate::compiler) state: PaleMossCarpetState,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct GateTemplateKey {
    pub(in crate::compiler) materials: [u32; 6],
    pub(in crate::compiler) orientation: u8,
    pub(in crate::compiler) open: bool,
    pub(in crate::compiler) in_wall: bool,
    pub(in crate::compiler) bamboo: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum SignTemplateKey {
    Standing {
        material: u32,
        rotation: u8,
    },
    Wall {
        material: u32,
        facing: u8,
    },
    HangingWall {
        material: u32,
        facing: u8,
    },
    HangingCeiling {
        material: u32,
        rotation: u8,
        attached: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) enum SignState {
    Standing { rotation: u8 },
    Wall { facing: u8 },
    HangingWall { facing: u8 },
    HangingCeiling { rotation: u8, attached: bool },
}

pub(in crate::compiler) fn push_model_template(
    quads: Vec<ModelQuad>,
    flags: u32,
    model_templates: &mut Vec<ModelTemplate>,
    model_quads: &mut Vec<ModelQuad>,
) -> Result<u32, AssetError> {
    debug_assert!(quads.len() <= 32);
    let template =
        u32::try_from(model_templates.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model template",
        })?;
    let quad_start =
        u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model quad",
        })?;
    let quad_count = u32::try_from(quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
        section: "model quad count",
    })?;
    model_templates.push(ModelTemplate {
        quad_start,
        quad_count,
        flags,
    });
    model_quads.extend(quads);
    Ok(template)
}

pub(in crate::compiler) fn intern_cuboid_template(
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
    template_by_key: &mut BTreeMap<CuboidTemplateKey, u32>,
    model_templates: &mut Vec<ModelTemplate>,
    model_quads: &mut Vec<ModelQuad>,
) -> Result<u32, AssetError> {
    let key = CuboidTemplateKey {
        materials,
        min,
        max,
    };
    if let Some(&template) = template_by_key.get(&key) {
        return Ok(template);
    }
    let template =
        u32::try_from(model_templates.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model template",
        })?;
    let quad_start =
        u32::try_from(model_quads.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "model quad",
        })?;
    model_templates.push(ModelTemplate {
        quad_start,
        quad_count: 6,
        flags: 0,
    });
    model_quads.extend(cuboid_quads(materials, min, max));
    template_by_key.insert(key, template);
    Ok(template)
}
