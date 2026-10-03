//! Equipment input, presentation, and cached-geometry types.

use std::sync::Arc;

use render::{ActorArtworkLocation, ActorRigSubmission, EntityRigId};

/// One stack an actor wears or holds, reduced to what drawing needs.
#[derive(Clone, Debug)]
pub(crate) struct WornItem {
    pub(crate) identifier: Arc<str>,
    pub(crate) metadata: u32,
    pub(crate) kind: HeldKind,
    pub(crate) dye_rgb: Option<u32>,
}

/// How a held stack is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeldKind {
    /// A flat compiled sprite.
    Sprite,
    /// A block item, by block visual id.
    Block(u32),
    Other,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum MeshKey {
    Sprite(usize),
    Block(u32),
    /// A session icon, by its index in the session layer.
    Session(usize),
    /// A session custom block item's cube sheet, by its index in the session layer.
    SessionBlock(usize),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ActorEquipmentInput {
    pub(crate) main: Option<WornItem>,
    pub(crate) off: Option<WornItem>,
    /// Helmet, chestplate, leggings, boots.
    pub(crate) armor: [Option<WornItem>; 4],
    pub(crate) sneaking: bool,
    pub(crate) sleeping: bool,
}

/// One extra instance plus the artwork page/layer its texture lives on.
pub(crate) struct EquipmentPresentation {
    pub(crate) submission: ActorRigSubmission,
    pub(crate) location: ActorArtworkLocation,
}

/// An ordinary item already in camera space, or an attachable on the avatar skeleton.
pub(crate) struct FirstPersonItem {
    pub(crate) presentation: EquipmentPresentation,
    pub(crate) camera_space: bool,
}

/// Which first-person arms the player render controller shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FirstPersonArms {
    pub(crate) right: bool,
    pub(crate) left: bool,
}

const FILLED_MAP: &str = "minecraft:filled_map";
const SHIELD: &str = "minecraft:shield";

impl FirstPersonArms {
    /// The right arm shows for an empty hand or a map; the left for a map in either hand (a shield
    /// in the off hand keeps it hidden). The use-item conditions await the item-use queries.
    pub(crate) fn for_hands(main: Option<&str>, off: Option<&str>) -> Self {
        Self {
            right: main.is_none_or(|main| main == FILLED_MAP),
            left: (main == Some(FILLED_MAP) && off != Some(SHIELD)) || off == Some(FILLED_MAP),
        }
    }

    /// Shows the right arm when the main-hand item drew nothing, so the rig still swings.
    pub(crate) fn with_undrawn_main(self, main_drawn: bool) -> Self {
        Self {
            right: self.right || !main_drawn,
            ..self
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ElytraStance {
    pub(super) sneaking: bool,
    pub(super) sleeping: bool,
}

pub(super) struct BodyBones {
    pub(super) names: Vec<Box<str>>,
    pub(super) right_item: Option<usize>,
    pub(super) left_item: Option<usize>,
    pub(super) head: Option<usize>,
}

pub(in crate::presentation::equipment) struct ArmorGeometry {
    pub(super) rig: EntityRigId,
    pub(super) names: Vec<Box<str>>,
    pub(super) pivots: Vec<[f32; 3]>,
}
