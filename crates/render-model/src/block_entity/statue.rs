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
        use assets::facts::{BlockModelVariant, CopperAge};
        let BlockModelVariant::Statue(age) = assets::facts::block_render_fact(name).variant? else {
            return None;
        };
        Some(match age {
            CopperAge::Unaffected => Self::Unaffected,
            CopperAge::Exposed => Self::Exposed,
            CopperAge::Weathered => Self::Weathered,
            CopperAge::Oxidized => Self::Oxidized,
        })
    }

    /// Selects the registry-owned oxidation domain for shared statue artwork.
    const fn age(self) -> assets::facts::CopperAge {
        use assets::facts::CopperAge;
        match self {
            Self::Unaffected => CopperAge::Unaffected,
            Self::Exposed => CopperAge::Exposed,
            Self::Weathered => CopperAge::Weathered,
            Self::Oxidized => CopperAge::Oxidized,
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
    let [body_path, eyes_path] =
        assets::block_entity_geometry::heads::statue_textures(model.oxidation.age());
    let matrix = model_matrix(block, [0.5, 0.0, 0.5], model.facing.yaw_degrees());
    let body = atlas.texture(&body_path, geometry.texture);
    let eyes = atlas.texture(&eyes_path, geometry.texture);
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

#[cfg(test)]
mod membership_tests {
    use super::*;
    /// Retains the old parser as an independent registry-wide membership oracle.
    fn previous(name: &str) -> Option<Oxidation> {
        let name = name.strip_prefix("minecraft:")?;
        let name = name.strip_prefix("waxed_").unwrap_or(name);
        Some(match name {
            "copper_golem_statue" => Oxidation::Unaffected,
            "exposed_copper_golem_statue" => Oxidation::Exposed,
            "weathered_copper_golem_statue" => Oxidation::Weathered,
            "oxidized_copper_golem_statue" => Oxidation::Oxidized,
            _ => return None,
        })
    }
    #[test]
    fn compiled_model_metadata_matches_previous_pinned_membership() {
        for record in assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap()
        {
            assert_eq!(
                Oxidation::from_block_name(&record.name),
                previous(&record.name),
                "{}",
                record.name
            );
        }
        assert_eq!(Oxidation::from_block_name("example:chest"), None);
    }
}
