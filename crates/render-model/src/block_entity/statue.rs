//! Copper golem statues: the entity geometry in one of its four poses, in the block's
//! oxidation texture with the eyes overlay at full brightness.
//!
//! The NBT pose encoding and the model's facing offset need native measurement.

use super::{
    atlas::BlockEntityAtlas,
    heads::HeadModels,
    mesh::{Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatuePose {
    Standing,
    Sitting,
    Star,
    Running,
}

impl StatuePose {
    /// Reads the `Pose` NBT as a name or, failing that, a pose index in the order above.
    #[must_use]
    pub fn from_nbt(name: Option<&str>, index: Option<i64>) -> Option<Self> {
        match (name, index) {
            (Some("standing"), _) | (None, Some(0)) => Some(Self::Standing),
            (Some("sitting"), _) | (None, Some(1)) => Some(Self::Sitting),
            (Some("star"), _) | (None, Some(2)) => Some(Self::Star),
            (Some("running"), _) | (None, Some(3)) => Some(Self::Running),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Standing => 0,
            Self::Sitting => 1,
            Self::Star => 2,
            Self::Running => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Oxidation {
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
}

impl Oxidation {
    /// The oxidation of a statue block name such as `minecraft:waxed_exposed_copper_golem_statue`.
    #[must_use]
    pub fn from_block_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("minecraft:")?;
        let name = name.strip_prefix("waxed_").unwrap_or(name);
        Some(match name {
            "copper_golem_statue" => Self::Unaffected,
            "exposed_copper_golem_statue" => Self::Exposed,
            "weathered_copper_golem_statue" => Self::Weathered,
            "oxidized_copper_golem_statue" => Self::Oxidized,
            _ => return None,
        })
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Unaffected => "",
            Self::Exposed => "_exposed",
            Self::Weathered => "_weathered",
            Self::Oxidized => "_oxidized",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatueModel {
    pub pose: StatuePose,
    pub oxidation: Oxidation,
    pub facing: Facing,
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    heads: &HeadModels,
    block: [i32; 3],
    model: &StatueModel,
) {
    let Some(geometry) = heads.statues[model.pose.index()].as_ref() else {
        return;
    };
    let suffix = model.oxidation.suffix();
    let matrix = model_matrix(block, [0.5, 0.0, 0.5], model.facing.yaw_degrees());
    let body = atlas.texture(
        &format!("textures/entity/copper_golem/copper_golem{suffix}"),
        geometry.texture,
    );
    let eyes = atlas.texture(
        &format!("textures/entity/copper_golem/copper_golem_eyes{suffix}"),
        geometry.texture,
    );
    for (texture, bright) in [(body, false), (eyes, true)] {
        let Some(texture) = texture else {
            continue;
        };
        let saved = builder.light;
        if bright {
            builder.light = 1.0;
        }
        for head_box in &geometry.boxes {
            builder.cuboid(
                Layer::Solid,
                &texture,
                matrix * head_box.matrix,
                head_box.spec,
                WHITE,
            );
        }
        builder.light = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_and_oxidation_parse_from_nbt_and_block_names() {
        assert_eq!(
            StatuePose::from_nbt(Some("star"), None),
            Some(StatuePose::Star)
        );
        assert_eq!(
            StatuePose::from_nbt(None, Some(3)),
            Some(StatuePose::Running)
        );
        assert_eq!(StatuePose::from_nbt(None, Some(9)), None);
        assert_eq!(
            Oxidation::from_block_name("minecraft:waxed_weathered_copper_golem_statue"),
            Some(Oxidation::Weathered)
        );
        assert_eq!(Oxidation::from_block_name("minecraft:copper_block"), None);
    }
}
