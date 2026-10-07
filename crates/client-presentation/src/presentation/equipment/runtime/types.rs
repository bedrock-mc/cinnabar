//! Equipment input, presentation, and cached-geometry types.

use std::sync::Arc;

use render::{ActorArtworkLocation, ActorRigSubmission};
use render_model::EntityRigId;

/// Source catalog, geometry, and selected image identify one attachable raster.
pub(super) type AttachableMeshKey = (bool, u32, Box<str>);

/// The immutable image frame and pose used by Java's independently placed raster draw.
#[derive(Clone)]
pub(super) struct JavaRasterFrame {
    pub image_to_rig: bevy::math::Mat4,
    pub rest: Arc<[render_model::RenderBoneTransform]>,
    pub normal_axis: bevy::math::Vec3,
}

/// One stack an actor wears or holds, reduced to what drawing needs.
#[derive(Clone, Debug)]
pub struct WornItem {
    pub identifier: Arc<str>,
    pub metadata: u32,
    /// Durability damage, when the stack carries it separately from its visual data value.
    pub damage: Option<u32>,
    pub kind: HeldKind,
    pub dye_rgb: Option<u32>,
    /// Enables the resource pack's enchanted material on worn equipment.
    pub enchanted: bool,
}

/// How a held stack is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeldKind {
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
pub struct ActorEquipmentInput {
    pub main: Option<WornItem>,
    pub off: Option<WornItem>,
    /// Helmet, chestplate, leggings, boots.
    pub armor: [Option<WornItem>; 4],
    pub sneaking: bool,
    pub sleeping: bool,
    /// Java 1.7 grips the main hand, when that mode poses this actor.
    pub java: Option<JavaGrip>,
}

/// How Java's third-person hand holds the main-hand item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JavaGrip {
    /// A sword in use is held as a block.
    pub blocking: bool,
}

/// One extra instance plus the artwork page/layer its texture lives on.
pub struct EquipmentPresentation {
    pub submission: ActorRigSubmission,
    pub location: ActorArtworkLocation,
}

/// An ordinary item already in camera space, or an attachable on the avatar skeleton.
pub struct FirstPersonItem {
    pub presentation: EquipmentPresentation,
    pub camera_space: bool,
    pub alpha_mode: render::HandItemAlphaMode,
    /// Camera from item space under Java's hand stack; the item's bones then sit at rest.
    pub java_camera: Option<bevy::math::Mat4>,
    /// Java's sprite-depth normal in this rig frame, used for legacy normal rescaling.
    pub java_normal_axis: bevy::math::Vec3,
}

/// Which first-person arms the player render controller shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstPersonArms {
    pub right: bool,
    pub left: bool,
}

const FILLED_MAP: &str = "minecraft:filled_map";
const SHIELD: &str = "minecraft:shield";

impl FirstPersonArms {
    /// The right arm shows for an empty hand or a map; the left for a map in either hand (a shield
    /// in the off hand keeps it hidden). The use-item conditions await the item-use queries.
    pub fn for_hands(main: Option<&str>, off: Option<&str>) -> Self {
        Self {
            right: main.is_none_or(|main| main == FILLED_MAP),
            left: (main == Some(FILLED_MAP) && off != Some(SHIELD)) || off == Some(FILLED_MAP),
        }
    }

    /// Shows the right arm when the main-hand item drew nothing, so the rig still swings.
    pub fn with_undrawn_main(self, main_drawn: bool) -> Self {
        Self {
            right: self.right || !main_drawn,
            ..self
        }
    }
}

/// Tick-owned actor queries and the sampled render fraction for worn attachables.
#[derive(Clone, Copy)]
pub struct EquipmentAnimation<'a> {
    pub owner: &'a client_world::ActorSnapshot,
    pub rig: &'a client_world::ActorRigSnapshot<'a>,
    pub frame_alpha: f32,
}

pub(super) struct BodyBones {
    pub(super) names: Vec<Box<str>>,
    pub(super) right_arm: Option<usize>,
    pub(super) right_item: Option<usize>,
    pub(super) left_item: Option<usize>,
    pub(super) head: Option<usize>,
}

pub(in crate::presentation::equipment) struct ArmorGeometry {
    pub(super) rig: EntityRigId,
    pub(super) names: Vec<Box<str>>,
    pub(super) pivots: Vec<[f32; 3]>,
    pub(super) binding_expressions: Vec<bool>,
}
