//! Dragon and piglin heads and copper golem statues, built from the pack's entity geometry rather
//! than authored here.

use assets::RuntimeEntityAssets;
use assets::block_entity_geometry::heads::{HeadModel, STATUE_GEOMETRIES, skull_mob_head};

use super::skull::SkullKind;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadModels {
    pub piglin: Option<HeadModel>,
    pub dragon: Option<HeadModel>,
    /// Copper golem statue poses: standing, sitting, star, running.
    pub statues: [Option<HeadModel>; 4],
}

impl HeadModels {
    #[must_use]
    pub fn from_assets(assets: &RuntimeEntityAssets) -> Self {
        let geometries = assets.geometries();
        let head = |kind: SkullKind| skull_mob_head(kind as u8)?.model(geometries);
        Self {
            piglin: head(SkullKind::Piglin),
            dragon: head(SkullKind::Dragon),
            statues: STATUE_GEOMETRIES.map(|identifier| {
                geometries
                    .iter()
                    .find(|geometry| geometry.identifier.as_ref() == identifier)
                    .and_then(|geometry| HeadModel::build_tree(geometry, None, 1.0, false))
            }),
        }
    }
}
