//! Turns a block entity's id, backing block state and NBT into what the renderer draws.

use std::sync::Arc;

use render::{
    BannerLayer, BannerModel, BannerMount, BedModel, BellAttachment, BlockEntityKind, ChestModel,
    ChestPair, ChestVariant, CopperAge, DecoratedPotModel, Facing, ItemFrameModel,
    MAX_BANNER_LAYERS, Oxidation, ShulkerModel, SignMount, SkullKind, SkullModel, SkullMount,
    SpawnerModel, StatueModel, StatuePose, banner_color, bed_color, pattern_texture, sherd_pattern,
    shulker_color_from_block_name,
};
use world::NbtCompound;

use super::{sign_text::SignTextSpec, state::BlockState};

const DEFAULT_SIGN_COLOR: i32 = -0x1000000;

/// An item stack held by a block entity.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HeldItem {
    pub(super) identifier: Arc<str>,
    pub(super) metadata: u32,
}

/// A block entity's drawing plan before per-frame animation and text resolution.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Template {
    Static(BlockEntityKind),
    ItemFrame {
        model: ItemFrameModel,
        item: Option<HeldItem>,
        rotation_steps: u8,
        /// The filled map's id when the framed item is a map.
        map_id: Option<i64>,
    },
    FlowerPot {
        plant: HeldItem,
    },
    Campfire {
        yaw_degrees: f32,
        items: [Option<HeldItem>; 4],
    },
    Bell {
        attachment: BellAttachment,
        direction: u8,
    },
    Conduit {
        active: bool,
        hunting: bool,
    },
    Beacon,
    Chest(ChestModel),
    Shulker(ShulkerModel),
    Sign {
        mount: SignMount,
        front: Option<SignTextSpec>,
        back: Option<SignTextSpec>,
    },
    EnchantTable,
}

/// Facing from either state spelling: `minecraft:cardinal_direction` or `facing_direction`.
fn facing(state: &BlockState) -> Option<Facing> {
    state
        .text("minecraft:cardinal_direction")
        .and_then(Facing::from_cardinal)
        .or_else(|| {
            state
                .int("facing_direction")
                .and_then(Facing::from_facing_direction)
        })
}

/// Sixteen-step ground rotation in degrees, 0 facing south.
fn ground_rotation(state: &BlockState) -> f32 {
    state
        .int("ground_sign_direction")
        .unwrap_or(0)
        .rem_euclid(16) as f32
        * 22.5
}

fn held_item(stack: &NbtCompound) -> Option<HeldItem> {
    let name = stack.string("Name").filter(|name| !name.is_empty())?;
    Some(HeldItem {
        identifier: Arc::from(name),
        metadata: stack
            .integer("Damage")
            .and_then(|damage| u32::try_from(damage).ok())
            .unwrap_or(0),
    })
}

fn chest_variant(block_name: &str) -> Option<ChestVariant> {
    let name = block_name.strip_prefix("minecraft:")?;
    let name = name.strip_prefix("waxed_").unwrap_or(name);
    Some(match name {
        "chest" => ChestVariant::Normal,
        "trapped_chest" => ChestVariant::Trapped,
        "copper_chest" => ChestVariant::Copper(CopperAge::Unaffected),
        "exposed_copper_chest" => ChestVariant::Copper(CopperAge::Exposed),
        "weathered_copper_chest" => ChestVariant::Copper(CopperAge::Weathered),
        "oxidized_copper_chest" => ChestVariant::Copper(CopperAge::Oxidized),
        _ => return None,
    })
}

fn chest_pair(position: [i32; 3], nbt: &NbtCompound) -> ChestPair {
    let (Some(pair_x), Some(pair_z)) = (nbt.integer("pairx"), nbt.integer("pairz")) else {
        return ChestPair::Single;
    };
    let (Ok(pair_x), Ok(pair_z)) = (i32::try_from(pair_x), i32::try_from(pair_z)) else {
        return ChestPair::Single;
    };
    // Without an explicit lead flag the lower coordinate leads, so exactly one half draws.
    let lead = nbt
        .boolean("pairlead")
        .unwrap_or((position[0], position[2]) < (pair_x, pair_z));
    if lead {
        ChestPair::Lead {
            partner: [pair_x, position[1], pair_z],
        }
    } else {
        ChestPair::Follower
    }
}

fn banner(block_name: &str, state: &BlockState, nbt: &NbtCompound) -> Option<Template> {
    let mount = match block_name.strip_prefix("minecraft:")? {
        "standing_banner" => BannerMount::Standing {
            rotation_degrees: ground_rotation(state),
        },
        "wall_banner" => BannerMount::Wall(facing(state)?),
        _ => return None,
    };
    let layers = nbt
        .list("Patterns")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let world::NbtValue::Compound(entry) = entry else {
                return None;
            };
            Some(BannerLayer {
                pattern: pattern_texture(entry.string("Pattern")?)?,
                color: banner_color(entry.integer("Color")?),
            })
        })
        .take(MAX_BANNER_LAYERS)
        .collect();
    Some(Template::Static(BlockEntityKind::Banner(BannerModel {
        mount,
        base: banner_color(nbt.integer("Base").unwrap_or(15)),
        layers,
    })))
}

fn sign_mount(block_name: &str, state: &BlockState) -> Option<SignMount> {
    let name = block_name.strip_prefix("minecraft:")?;
    if name.ends_with("hanging_sign") {
        return Some(if state.int("attached_bit") == Some(1) {
            SignMount::Hanging {
                rotation_degrees: ground_rotation(state),
            }
        } else {
            SignMount::HangingWall(facing(state)?)
        });
    }
    if name.ends_with("wall_sign") {
        return Some(SignMount::Wall(facing(state)?));
    }
    name.ends_with("standing_sign")
        .then(|| SignMount::Standing {
            rotation_degrees: ground_rotation(state),
        })
}

fn sign_face(face: &NbtCompound) -> Option<SignTextSpec> {
    let spec = SignTextSpec {
        text: face.string("Text")?.to_owned(),
        color_argb: face
            .integer("SignTextColor")
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(DEFAULT_SIGN_COLOR),
        glowing: face.boolean("IgnoreLighting").unwrap_or(false),
        hide_glow_outline: face.boolean("HideGlowOutline").unwrap_or(false),
    };
    spec.is_visible().then_some(spec)
}

fn sign(block_name: &str, state: &BlockState, nbt: &NbtCompound) -> Option<Template> {
    let mount = sign_mount(block_name, state)?;
    let (front, back) = match (nbt.compound("FrontText"), nbt.compound("BackText")) {
        (None, None) => (
            // Pre-1.19.80 signs carry one face's text at the root.
            sign_face(nbt),
            None,
        ),
        (front, back) => (front.and_then(sign_face), back.and_then(sign_face)),
    };
    (front.is_some() || back.is_some()).then_some(Template::Sign { mount, front, back })
}

/// Plans the draw for one block entity, or `None` when nothing should be drawn.
pub(super) fn describe(
    id: &str,
    block_name: &str,
    state: &BlockState,
    nbt: &NbtCompound,
    position: [i32; 3],
) -> Option<Template> {
    match id {
        "Chest" => Some(Template::Chest(ChestModel {
            variant: chest_variant(block_name)?,
            facing: facing(state).unwrap_or(Facing::North),
            pair: chest_pair(position, nbt),
            lid: 0.0,
        })),
        "EnderChest" => Some(Template::Chest(ChestModel {
            variant: ChestVariant::Ender,
            facing: facing(state).unwrap_or(Facing::North),
            pair: ChestPair::Single,
            lid: 0.0,
        })),
        "ShulkerBox" => Some(Template::Shulker(ShulkerModel {
            color: shulker_color_from_block_name(block_name)?,
            facing: nbt
                .integer("facing")
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value < 6)
                .unwrap_or(1),
            open: 0.0,
        })),
        "Skull" => {
            let kind = SkullKind::from_nbt(nbt.integer("SkullType")?)?;
            let mount = match state
                .int("facing_direction")
                .and_then(Facing::from_facing_direction)
            {
                Some(wall) => SkullMount::Wall(wall),
                None => SkullMount::Floor {
                    rotation_degrees: nbt.float("Rotation").unwrap_or(0.0),
                },
            };
            Some(Template::Static(BlockEntityKind::Skull(SkullModel {
                kind,
                mount,
            })))
        }
        "Bed" => Some(Template::Static(BlockEntityKind::Bed(BedModel {
            color: bed_color(nbt.integer("color")?)?,
            head: state.int("head_piece_bit") == Some(1),
            direction: u8::try_from(state.int("direction")?.rem_euclid(4)).ok()?,
        }))),
        "Banner" => banner(block_name, state, nbt),
        "Sign" | "HangingSign" => sign(block_name, state, nbt),
        "EnchantTable" => Some(Template::EnchantTable),
        "Lectern" => Some(Template::Static(BlockEntityKind::Lectern {
            facing_yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            has_book: nbt.boolean("hasBook").unwrap_or(false),
        })),
        "Bell" => Some(Template::Bell {
            attachment: state
                .text("attachment")
                .and_then(BellAttachment::from_state)
                .unwrap_or(BellAttachment::Standing),
            direction: state
                .int("direction")
                .and_then(|value| u8::try_from(value.rem_euclid(4)).ok())
                .unwrap_or(0),
        }),
        "Beacon" => (nbt.integer("Levels").unwrap_or(0) > 0).then_some(Template::Beacon),
        "ItemFrame" | "GlowItemFrame" => {
            // The state stores the face toward the wall; the frame looks the other way.
            let toward_wall = u8::try_from(state.int("facing_direction")?).ok()?;
            (toward_wall < 6).then(|| Template::ItemFrame {
                model: ItemFrameModel {
                    glow: id == "GlowItemFrame",
                    outward: toward_wall ^ 1,
                    map: None,
                },
                item: nbt.compound("Item").and_then(held_item),
                map_id: nbt
                    .compound("Item")
                    .and_then(|stack| stack.compound("tag"))
                    .and_then(|tag| tag.integer("map_uuid")),
                rotation_steps: nbt
                    .integer("ItemRotation")
                    .and_then(|steps| u8::try_from(steps.rem_euclid(8)).ok())
                    .unwrap_or(0),
            })
        }
        "FlowerPot" => nbt
            .compound("PlantBlock")
            .and_then(|plant| plant.string("name"))
            .filter(|name| !name.is_empty() && *name != "minecraft:air")
            .map(|name| Template::FlowerPot {
                plant: HeldItem {
                    identifier: Arc::from(name),
                    metadata: 0,
                },
            }),
        "Campfire" => Some(Template::Campfire {
            yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            items: std::array::from_fn(|slot| {
                nbt.compound(&format!("Item{}", slot + 1))
                    .and_then(held_item)
            }),
        }),
        "Conduit" => Some(Template::Conduit {
            active: nbt.boolean("Active").unwrap_or(false),
            hunting: nbt.integer("Target").is_some_and(|target| target != -1),
        }),
        "DecoratedPot" => {
            let mut sherds: [Option<String>; 4] = Default::default();
            for (slot, entry) in nbt
                .list("sherds")
                .unwrap_or_default()
                .iter()
                .take(4)
                .enumerate()
            {
                if let world::NbtValue::String(item) = entry {
                    sherds[slot] = sherd_pattern(item);
                }
            }
            Some(Template::Static(BlockEntityKind::DecoratedPot(
                DecoratedPotModel {
                    facing: facing(state).unwrap_or(Facing::North),
                    sherds,
                },
            )))
        }
        "CopperGolemStatue" => Some(Template::Static(BlockEntityKind::Statue(StatueModel {
            pose: StatuePose::from_nbt(nbt.string("Pose"), nbt.integer("Pose"))?,
            oxidation: Oxidation::from_block_name(block_name)?,
            facing: facing(state).unwrap_or(Facing::North),
        }))),
        "MobSpawner" => nbt
            .string("EntityIdentifier")
            .filter(|mob| !mob.is_empty())
            .map(|mob| {
                Template::Static(BlockEntityKind::Spawner(SpawnerModel {
                    mob: Arc::from(mob),
                }))
            }),
        "EndPortal" => Some(Template::Static(BlockEntityKind::EndPortal)),
        "EndGateway" => Some(Template::Static(BlockEntityKind::EndGateway)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nbt(build: impl FnOnce(&mut Vec<u8>)) -> NbtCompound {
        let mut bytes = vec![10, 0];
        build(&mut bytes);
        bytes.push(0);
        world::BlockEntityNbt::decode_prefix(&bytes)
            .unwrap()
            .0
            .parse()
            .unwrap()
    }

    fn int_tag(out: &mut Vec<u8>, key: &str, zigzag: u8) {
        out.push(3);
        out.push(key.len() as u8);
        out.extend_from_slice(key.as_bytes());
        out.push(zigzag);
    }

    fn state(json: &str) -> BlockState {
        BlockState::parse(json)
    }

    #[test]
    fn review_campfire_items_keep_their_nbt_slots() {
        let compound = nbt(|out| {
            out.extend([10, 5]);
            out.extend(b"Item4");
            out.extend([8, 4]);
            out.extend(b"Name");
            let name = b"minecraft:apple";
            out.push(name.len() as u8);
            out.extend(name);
            out.push(0);
        });
        let Some(Template::Campfire { items, .. }) = describe(
            "Campfire",
            "minecraft:campfire",
            &BlockState::default(),
            &compound,
            [0; 3],
        ) else {
            panic!("expected campfire");
        };
        assert!(items[..3].iter().all(Option::is_none));
        assert_eq!(
            items[3].as_ref().unwrap().identifier.as_ref(),
            "minecraft:apple"
        );
    }

    #[test]
    fn chest_pair_leads_by_flag_or_lower_coordinate() {
        // pairx = 5 (zigzag 10), pairz = 0.
        let plain = nbt(|out| {
            int_tag(out, "pairx", 10);
            int_tag(out, "pairz", 0);
        });
        assert_eq!(
            chest_pair([4, 64, 0], &plain),
            ChestPair::Lead {
                partner: [5, 64, 0]
            }
        );
        assert_eq!(chest_pair([6, 64, 0], &plain), ChestPair::Follower);
        assert_eq!(chest_pair([4, 64, 0], &nbt(|_| {})), ChestPair::Single);
    }

    #[test]
    fn chest_variants_and_facing_resolve_from_the_block() {
        let template = describe(
            "Chest",
            "minecraft:waxed_exposed_copper_chest",
            &state(r#"{"minecraft:cardinal_direction":{"type":"string","value":"east"}}"#),
            &nbt(|_| {}),
            [0, 0, 0],
        );
        let Some(Template::Chest(model)) = template else {
            panic!("chest expected");
        };
        assert_eq!(model.variant, ChestVariant::Copper(CopperAge::Exposed));
        assert_eq!(model.facing, Facing::East);
        assert!(
            describe(
                "Chest",
                "minecraft:stone",
                &BlockState::default(),
                &nbt(|_| {}),
                [0; 3]
            )
            .is_none()
        );
    }

    #[test]
    fn sign_text_needs_visible_characters_on_some_face() {
        let empty = nbt(|_| {});
        let standing = state(r#"{"ground_sign_direction":4}"#);
        assert!(describe("Sign", "minecraft:standing_sign", &standing, &empty, [0; 3]).is_none());
        let text = nbt(|out| {
            out.push(8);
            out.push(4);
            out.extend_from_slice(b"Text");
            out.push(2);
            out.extend_from_slice(b"hi");
        });
        let Some(Template::Sign { mount, front, back }) =
            describe("Sign", "minecraft:standing_sign", &standing, &text, [0; 3])
        else {
            panic!("legacy root text draws on the front");
        };
        assert_eq!(
            mount,
            SignMount::Standing {
                rotation_degrees: 90.0
            }
        );
        assert_eq!(front.unwrap().text, "hi");
        assert!(back.is_none());
    }

    #[test]
    fn beds_read_color_from_nbt_and_half_and_direction_from_state() {
        // color byte 14 is red.
        let nbt = nbt(|out| {
            out.push(1);
            out.push(5);
            out.extend_from_slice(b"color");
            out.push(14);
        });
        let template = describe(
            "Bed",
            "minecraft:bed",
            &state(r#"{"direction":{"type":"int","value":3},"head_piece_bit":true}"#),
            &nbt,
            [0; 3],
        );
        assert_eq!(
            template,
            Some(Template::Static(BlockEntityKind::Bed(BedModel {
                color: "red",
                head: true,
                direction: 3,
            })))
        );
    }

    #[test]
    fn item_frames_face_away_from_the_wall_the_state_names() {
        // facing_direction 2 (north) is the wall side, so the frame looks south (3).
        let template = describe(
            "GlowItemFrame",
            "minecraft:glow_frame",
            &state(r#"{"facing_direction":{"type":"int","value":2}}"#),
            &nbt(|_| {}),
            [0; 3],
        );
        let Some(Template::ItemFrame { model, item, .. }) = template else {
            panic!("frame expected");
        };
        assert!(model.glow && model.outward == 3 && item.is_none());
    }

    #[test]
    fn ids_without_a_renderer_draw_nothing() {
        assert!(
            describe(
                "Furnace",
                "minecraft:furnace",
                &BlockState::default(),
                &nbt(|_| {}),
                [0; 3]
            )
            .is_none()
        );
    }
}
