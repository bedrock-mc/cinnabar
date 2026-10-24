//! Equipment input, presentation, and cached-geometry types.

use std::sync::Arc;

use render_model::EntityRigId;
use render_model::{ActorArtworkLocation, ActorRigSubmission};

/// Geometry namespace and resolved artwork page/layer identify each texture-derived mesh.
pub(super) type AttachableMeshKey = (bool, u32, (u16, u32));

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
    /// Image identity a held map samples, independent of its atlas icon variant.
    pub map_id: Option<i64>,
    /// Loaded projectile admitted for this stack, independent of display metadata.
    pub charged_projectile: Option<assets::item_use::ItemProjectileRole>,
    /// Grip admitted by the numeric item registry, copied from the canonical stack.
    pub grip: assets::item::grip::ItemGrip,
    /// Use and animation facts from the numerical registry admission.
    pub use_facts: client_world::EffectiveItemUse,
    pub identifier: Arc<str>,
    pub metadata: u32,
    /// Durability damage, when the stack carries it separately from its visual data value.
    pub damage: Option<u32>,
    pub kind: HeldKind,
    pub dye_rgb: Option<u32>,
    /// Actual stack enchantment state used by semantic item queries.
    pub enchanted: bool,
    /// Intrinsic shine admitted independently of stack enchantments.
    pub glint: assets::item::glint::ItemGlint,
    /// Data value, block and tags whose change re-equips a held stack.
    pub variant: actor_animation::StackVariant,
}

impl WornItem {
    /// Combines actual enchantments with the admitted intrinsic choice for visual shine.
    pub fn has_glint(&self) -> bool {
        self.enchanted || self.glint.enabled().unwrap_or(false)
    }
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
    /// A session custom block item's material sheet, by its index in the session layer.
    SessionBlock(usize),
}

#[derive(Clone, Debug, Default)]
pub struct ActorEquipmentInput {
    /// The equipment belongs to the player in this game window.
    pub is_local_player: bool,
    pub main: Option<WornItem>,
    pub off: Option<WornItem>,
    /// Helmet, chestplate, leggings, boots.
    pub armor: [Option<WornItem>; 4],
    /// Equipment replicated in the separate body slot.
    pub body: Option<WornItem>,
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
#[derive(Clone)]
pub struct EquipmentPresentation {
    pub submission: ActorRigSubmission,
    pub location: ActorArtworkLocation,
}

/// Drawn equipment and the chest routing decision used by later cape composition.
pub struct EquipmentLayers<'a> {
    pub layers: &'a mut [EquipmentPresentation],
    pub wearing_elytra: bool,
}

/// An ordinary item already in camera space, or an attachable on the avatar skeleton.
pub struct FirstPersonItem {
    pub presentation: EquipmentPresentation,
    pub camera_space: bool,
    pub alpha_mode: render_model::HandItemAlphaMode,
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

const FILLED_MAP: &str = assets::facts::FILLED_MAP_IDENTIFIER;
const SHIELD: &str = assets::facts::VanillaItem::Shield.identifier();

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

/// Actor queries and sampled render timing for animated equipment.
#[derive(Clone, Copy)]
pub struct EquipmentAnimation<'a> {
    pub owner: &'a client_world::ActorSnapshot,
    pub rig: &'a actor_animation::ActorRigSnapshot<'a>,
    pub frame_alpha: f32,
    pub delta_seconds: f32,
}

#[derive(Clone, Eq, PartialEq)]
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

/// A controller's armour draw, including poses for bones absent from the owner's model.
pub(super) struct WornRender {
    pub texture: Box<str>,
    pub geometry: Box<str>,
    pub layer: actor_animation::RenderTextureLayer,
    pub pose: Arc<[render_model::RenderBoneTransform]>,
}

impl EquipmentAnimation<'_> {
    /// Supplies held selection and animation with identical hand, owner and use inputs.
    pub(super) fn held_input<'a>(
        self,
        equipment: &'a ActorEquipmentInput,
        off_hand: bool,
    ) -> actor_animation::AttachableAnimationInput<'a> {
        actor_animation::AttachableAnimationInput {
            off_hand,
            held_item_scale: self.rig.held_item_scale,
            worn: false,
            ..self.worn_input(equipment, assets::ArmorSlot::Helmet, &[])
        }
    }

    /// Supplies the same main-hand use clock to worn selection and controller evaluation.
    pub(super) fn worn_input<'a>(
        self,
        equipment: &'a ActorEquipmentInput,
        slot: assets::ArmorSlot,
        variables: &'a [(&'a str, f32)],
    ) -> actor_animation::AttachableAnimationInput<'a> {
        let elapsed = self
            .owner
            .is_using_item()
            .then_some(self.rig.hand[1].use_ticks);
        let use_facts = equipment
            .main
            .as_ref()
            .map(|main| main.use_facts)
            .unwrap_or_default();
        let duration = use_facts.ticks.unwrap_or_default();
        equipment.attachable_input(actor_animation::AttachableAnimationInput {
            worn: true,
            worn_slot: slot as u8,
            frame_alpha: self.frame_alpha,
            delta_seconds: Some(self.delta_seconds),
            use_elapsed_ticks: elapsed,
            max_use_ticks: duration,
            animation_frame: inventory::ranged_animation_frame(
                use_facts.animation,
                elapsed,
                duration,
            ),
            owner_variables: variables,
            ..Default::default()
        })
    }
}
