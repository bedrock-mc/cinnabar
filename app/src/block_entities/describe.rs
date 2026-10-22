//! Turns a block entity's id, backing block state and NBT into what the renderer draws.

use assets::{
    banner::{MAX_BANNER_LAYERS, color_linear as banner_color, pattern_texture},
    block_entity_geometry::shulker_color_from_block_name,
    facts::{BlockEntityBacking, BlockEntityMount, BlockEntityRenderer, BlockProperty},
};
use std::sync::Arc;

use render_model::{
    BannerLayer, BannerModel, BannerMount, BedModel, BellAttachment, BlockEntityKind, ChestModel,
    ChestPair, ChestVariant, DecoratedPotModel, Facing, ITEM_FRAME_ROTATION_STEP_DEGREES,
    ItemFrameModel, Oxidation, ShulkerModel, SignMount, SkullKind, SkullModel, SkullMount,
    StatueModel, StatuePose, bed_color, sherd_pattern,
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

/// A retained plant block with all typed state selectors used to resolve its world identity.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlantBlock {
    pub(super) identifier: Arc<str>,
    pub(super) states: serde_json::Map<String, serde_json::Value>,
}

/// Reads only usable block-state tag types; malformed plants do not become inventory icons.
fn plant_block(plant: &NbtCompound) -> Option<PlantBlock> {
    let name = plant
        .string("name")
        .filter(|name| !name.is_empty() && *name != assets::facts::block::AIR_IDENTIFIER)?;
    let mut states = serde_json::Map::new();
    if let Some(compound) = plant.compound("states") {
        for (key, value) in compound.iter() {
            let typed = match value {
                world::NbtValue::Byte(value) => serde_json::json!({"type":"byte", "value":value}),
                world::NbtValue::Int(value) => serde_json::json!({"type":"int", "value":value}),
                world::NbtValue::String(value) => {
                    serde_json::json!({"type":"string", "value":value})
                }
                _ => return None,
            };
            states.insert(key.to_owned(), typed);
        }
    } else if plant.get("states").is_some() {
        return None;
    }
    Some(PlantBlock {
        identifier: Arc::from(name),
        states,
    })
}

/// A block entity's drawing plan before per-frame animation and text resolution.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Template {
    Static(BlockEntityKind),
    ItemFrame {
        model: ItemFrameModel,
        item: Option<HeldItem>,
        rotation_degrees: f32,
        /// The filled map's id when the framed item is a map.
        map_id: Option<i64>,
    },
    FlowerPot {
        plant: PlantBlock,
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
        .text(BlockProperty::MinecraftCardinalDirection.name())
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

/// Reads modern degree rotations and legacy integral turns, defaulting unusable data to zero.
fn frame_rotation(nbt: &NbtCompound) -> f32 {
    if let Some(turns) = nbt.integer("ItemRotation") {
        return turns.rem_euclid(8) as f32 * ITEM_FRAME_ROTATION_STEP_DEGREES;
    }
    nbt.float("ItemRotation")
        .filter(|degrees| degrees.is_finite())
        .map(|degrees| degrees.rem_euclid(8.0 * ITEM_FRAME_ROTATION_STEP_DEGREES))
        .unwrap_or(0.0)
}

/// Reads a usable retained item stack.
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

/// Selects one leading half from a valid pair.
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

/// Builds banner layers for the compiled mount.
fn banner(
    backing: BlockEntityBacking<'_>,
    state: &BlockState,
    nbt: &NbtCompound,
) -> Option<Template> {
    let mount = match backing.mount {
        BlockEntityMount::Standing => BannerMount::Standing {
            rotation_degrees: ground_rotation(state),
        },
        BlockEntityMount::Wall => BannerMount::Wall(facing(state)?),
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

/// Resolves the compiled mount using the retained state's facing and rotation.
fn sign_mount(backing: BlockEntityBacking<'_>, state: &BlockState) -> Option<SignMount> {
    Some(match backing.mount {
        BlockEntityMount::Hanging => {
            if state.int(BlockProperty::AttachedBit.name()) == Some(1) {
                SignMount::Hanging {
                    rotation_degrees: ground_rotation(state),
                }
            } else {
                SignMount::HangingWall(facing(state)?)
            }
        }
        BlockEntityMount::Wall => SignMount::Wall(facing(state)?),
        BlockEntityMount::Standing => SignMount::Standing {
            rotation_degrees: ground_rotation(state),
        },
        BlockEntityMount::None => return None,
    })
}

/// Keeps visible text and its retained appearance.
fn sign_face(face: &NbtCompound) -> Option<SignTextSpec> {
    let spec = SignTextSpec::new(
        face.string("Text")?.to_owned(),
        face.integer("SignTextColor")
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(DEFAULT_SIGN_COLOR),
        face.boolean("IgnoreLighting").unwrap_or(false),
        face.boolean("HideGlowOutline").unwrap_or(false),
    );
    spec.is_visible().then_some(spec)
}

/// Plans the visible faces of a sign on its compiled mount.
fn sign(
    backing: BlockEntityBacking<'_>,
    state: &BlockState,
    nbt: &NbtCompound,
) -> Option<Template> {
    let mount = sign_mount(backing, state)?;
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
    backing: BlockEntityBacking<'_>,
    block_name: &str,
    state: &BlockState,
    nbt: &NbtCompound,
    position: [i32; 3],
) -> Option<Template> {
    if backing.entity.nbt_id != id {
        return None;
    }
    match backing.entity.renderer {
        BlockEntityRenderer::Chest => Some(Template::Chest(ChestModel {
            variant: ChestVariant::from_block_name(block_name)?,
            facing: facing(state).unwrap_or(Facing::North),
            pair: chest_pair(position, nbt),
            lid: 0.0,
        })),
        BlockEntityRenderer::EnderChest => Some(Template::Chest(ChestModel {
            variant: ChestVariant::Ender,
            facing: facing(state).unwrap_or(Facing::North),
            pair: ChestPair::Single,
            lid: 0.0,
        })),
        BlockEntityRenderer::Shulker => Some(Template::Shulker(ShulkerModel {
            color: shulker_color_from_block_name(block_name)?,
            facing: nbt
                .integer("facing")
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value < 6)
                .unwrap_or(1),
            open: 0.0,
        })),
        BlockEntityRenderer::Skull => {
            let kind = SkullKind::from_block_identifier(block_name)?;
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
        BlockEntityRenderer::Bed => Some(Template::Static(BlockEntityKind::Bed(BedModel {
            color: bed_color(nbt.integer("color")?)?,
            head: state.int(BlockProperty::HeadPieceBit.name()) == Some(1),
            direction: u8::try_from(state.int("direction")?.rem_euclid(4)).ok()?,
        }))),
        BlockEntityRenderer::Chalkboard => super::chalkboard::model(state, nbt, position)
            .map(|model| Template::Static(BlockEntityKind::Chalkboard(model))),
        BlockEntityRenderer::Banner => banner(backing, state, nbt),
        BlockEntityRenderer::Sign => sign(backing, state, nbt),
        BlockEntityRenderer::EnchantTable => Some(Template::EnchantTable),
        BlockEntityRenderer::Lectern => Some(Template::Static(BlockEntityKind::Lectern {
            facing_yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            has_book: nbt.boolean("hasBook").unwrap_or(false),
        })),
        BlockEntityRenderer::Bell => Some(Template::Bell {
            attachment: state
                .text("attachment")
                .and_then(BellAttachment::from_state)
                .unwrap_or(BellAttachment::Standing),
            direction: state
                .int("direction")
                .and_then(|value| u8::try_from(value.rem_euclid(4)).ok())
                .unwrap_or(0),
        }),
        BlockEntityRenderer::Beacon => {
            (nbt.integer("Levels").unwrap_or(0) > 0).then_some(Template::Beacon)
        }
        BlockEntityRenderer::ItemFrame | BlockEntityRenderer::GlowItemFrame => {
            let outward = u8::try_from(state.int("facing_direction")?).ok()?;
            (outward < 6).then(|| Template::ItemFrame {
                model: ItemFrameModel {
                    glow: backing.entity.renderer == BlockEntityRenderer::GlowItemFrame,
                    outward,
                    map: None,
                },
                item: nbt.compound("Item").and_then(held_item),
                map_id: nbt
                    .compound("Item")
                    .and_then(|stack| stack.compound("tag"))
                    .and_then(|tag| tag.integer("map_uuid")),
                rotation_degrees: frame_rotation(nbt),
            })
        }
        BlockEntityRenderer::FlowerPot => nbt
            .compound("PlantBlock")
            .and_then(plant_block)
            .map(|plant| Template::FlowerPot { plant }),
        BlockEntityRenderer::Campfire => Some(Template::Campfire {
            yaw_degrees: facing(state).unwrap_or(Facing::North).yaw_degrees(),
            items: std::array::from_fn(|slot| {
                nbt.compound(&format!("Item{}", slot + 1))
                    .and_then(held_item)
            }),
        }),
        BlockEntityRenderer::Conduit => Some(Template::Conduit {
            active: nbt.boolean("Active").unwrap_or(false),
            hunting: nbt.integer("Target").is_some_and(|target| target != -1),
        }),
        BlockEntityRenderer::DecoratedPot => {
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
        BlockEntityRenderer::Statue => {
            Some(Template::Static(BlockEntityKind::Statue(StatueModel {
                pose: StatuePose::from_nbt(nbt.string("Pose"), nbt.integer("Pose"))?,
                oxidation: Oxidation::from_block_name(block_name)?,
                facing: facing(state).unwrap_or(Facing::North),
            })))
        }
        BlockEntityRenderer::EndPortal => Some(Template::Static(BlockEntityKind::EndPortal)),
        BlockEntityRenderer::EndGateway => Some(Template::Static(BlockEntityKind::EndGateway)),
        BlockEntityRenderer::None | BlockEntityRenderer::MobSpawner => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::block_entity::CopperAge;

    /// A retained plant with typed NBT states, independent of inventory item metadata.
    fn potted_nbt(name: &str, states: NbtCompound) -> NbtCompound {
        let mut plant = NbtCompound::default();
        plant.insert("name", world::NbtValue::String(name.into()));
        plant.insert("states", world::NbtValue::Compound(states));
        let mut root = NbtCompound::default();
        root.insert("PlantBlock", world::NbtValue::Compound(plant));
        root
    }

    #[test]
    fn flower_pot_preserves_typed_plant_state_selectors() {
        let mut states = NbtCompound::default();
        states.insert("sapling_type", world::NbtValue::String("spruce".into()));
        states.insert("age_bit", world::NbtValue::Byte(0));
        states.insert("growth", world::NbtValue::Int(2));
        let Some(Template::FlowerPot { plant }) = describe(
            "FlowerPot",
            "minecraft:flower_pot",
            &BlockState::default(),
            &potted_nbt("minecraft:sapling", states),
            [0; 3],
        ) else {
            panic!("plant expected")
        };
        assert_eq!(plant.identifier.as_ref(), "minecraft:sapling");
        assert_eq!(
            plant.states["sapling_type"],
            serde_json::json!({"type":"string", "value":"spruce"})
        );
        assert_eq!(
            plant.states["age_bit"],
            serde_json::json!({"type":"byte", "value":0})
        );
        assert_eq!(
            plant.states["growth"],
            serde_json::json!({"type":"int", "value":2})
        );
    }

    #[test]
    fn flower_pot_skips_empty_invalid_and_stale_contents() {
        for name in ["", "minecraft:air"] {
            assert!(
                describe(
                    "FlowerPot",
                    "minecraft:flower_pot",
                    &BlockState::default(),
                    &potted_nbt(name, NbtCompound::default()),
                    [0; 3]
                )
                .is_none()
            );
        }
        let root = potted_nbt("minecraft:poppy", NbtCompound::default());
        assert!(
            describe(
                "FlowerPot",
                "minecraft:stone",
                &BlockState::default(),
                &root,
                [0; 3]
            )
            .is_none()
        );
        let mut states = NbtCompound::default();
        states.insert("growth", world::NbtValue::Float(2.0));
        assert!(
            describe(
                "FlowerPot",
                "minecraft:flower_pot",
                &BlockState::default(),
                &potted_nbt("minecraft:poppy", states),
                [0; 3]
            )
            .is_none()
        );
    }

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

    /// Resolves a test's named backing from the real pinned palette.
    fn describe(
        id: &str,
        name: &str,
        state: &BlockState,
        nbt: &NbtCompound,
        position: [i32; 3],
    ) -> Option<Template> {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let record = records.iter().find(|r| r.name.as_ref() == name)?;
        let backing = assets::facts::BlockEntityFacts::pinned().get(record.sequential_id)?;
        super::describe(id, backing, name, state, nbt, position)
    }

    #[test]
    fn mismatched_block_entity_id_cannot_draw_on_a_different_backing() {
        let empty = NbtCompound::default();
        for (id, name) in [
            ("Banner", "minecraft:stone"),
            ("EnderChest", "minecraft:chest"),
            ("MobSpawner", "minecraft:trial_spawner"),
            ("Sign", "minecraft:oak_hanging_sign"),
        ] {
            assert!(describe(id, name, &BlockState::default(), &empty, [0; 3]).is_none());
        }
    }

    #[test]
    fn a_broken_enchanting_table_leaves_no_floating_book() {
        let empty = nbt(|_| {});
        let none = state("{}");
        assert!(matches!(
            describe(
                "EnchantTable",
                "minecraft:enchanting_table",
                &none,
                &empty,
                [0; 3]
            ),
            Some(Template::EnchantTable)
        ));
        assert!(describe("EnchantTable", "minecraft:air", &none, &empty, [0; 3]).is_none());
        assert!(describe("EnchantTable", "minecraft:stone", &none, &empty, [0; 3]).is_none());
    }

    #[test]
    fn current_player_head_identity_overrides_absent_or_stale_skull_type() {
        for compound in [nbt(|_| {}), nbt(|out| int_tag(out, "SkullType", 0))] {
            let template = describe(
                "Skull",
                "minecraft:player_head",
                &BlockState::default(),
                &compound,
                [0; 3],
            );
            assert!(matches!(
                template,
                Some(Template::Static(BlockEntityKind::Skull(SkullModel {
                    kind: SkullKind::Player,
                    ..
                })))
            ));
        }
        let legacy = nbt(|out| int_tag(out, "SkullType", 6));
        assert!(
            describe(
                "Skull",
                "minecraft:skull",
                &BlockState::default(),
                &legacy,
                [0; 3]
            )
            .is_none()
        );
        assert!(
            describe(
                "Skull",
                "custom:player_head",
                &BlockState::default(),
                &legacy,
                [0; 3]
            )
            .is_none()
        );
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
        assert_eq!(
            front,
            Some(SignTextSpec::new(
                "hi".into(),
                DEFAULT_SIGN_COLOR,
                false,
                false
            ))
        );
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
    fn item_frames_preserve_all_six_outward_facing_states() {
        for (id, name, glow) in [
            ("ItemFrame", "minecraft:frame", false),
            ("GlowItemFrame", "minecraft:glow_frame", true),
        ] {
            let models = (0..6)
                .map(|outward| {
                    let template = describe(
                        id,
                        name,
                        &state(&format!(
                            r#"{{"facing_direction":{{"type":"int","value":{outward}}}}}"#
                        )),
                        &nbt(|_| {}),
                        [0; 3],
                    );
                    let Some(Template::ItemFrame { model, item, .. }) = template else {
                        panic!("frame expected");
                    };
                    assert!(item.is_none());
                    model
                })
                .collect::<Vec<_>>();
            for (outward, model) in models.iter().enumerate() {
                assert_eq!(usize::from(model.outward), outward);
                assert_eq!(model.glow, glow);
            }
        }
    }

    #[test]
    fn item_frame_rotation_accepts_float_degrees_and_legacy_byte_turns() {
        for step in 0..8_u8 {
            for floating in [false, true] {
                let rotation = nbt(|out| {
                    out.push(if floating { 5 } else { 1 });
                    out.push(12);
                    out.extend_from_slice(b"ItemRotation");
                    if floating {
                        out.extend_from_slice(
                            &(f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES).to_le_bytes(),
                        );
                    } else {
                        out.push(step);
                    }
                    out.extend_from_slice(&[10, 4]);
                    out.extend_from_slice(b"Item");
                    out.extend_from_slice(&[8, 4]);
                    out.extend_from_slice(b"Name");
                    out.push(15);
                    out.extend_from_slice(b"minecraft:clock");
                    out.push(0);
                });
                let Some(Template::ItemFrame {
                    rotation_degrees,
                    item,
                    ..
                }) = describe(
                    "ItemFrame",
                    "minecraft:frame",
                    &state(r#"{"facing_direction":{"type":"int","value":2}}"#),
                    &rotation,
                    [0; 3],
                )
                else {
                    panic!("frame expected");
                };
                assert_eq!(item.unwrap().identifier.as_ref(), "minecraft:clock");
                assert_eq!(
                    rotation_degrees,
                    f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES,
                    "float degrees: {floating}"
                );
            }
        }
    }

    #[test]
    fn item_frame_rotation_retains_fractional_degrees_and_defaults_unusable_values() {
        for (degrees, expected) in [
            (22.5_f32, 22.5),
            (-22.5, 337.5),
            (382.5, 22.5),
            (f32::NAN, 0.0),
            (f32::INFINITY, 0.0),
            (f32::NEG_INFINITY, 0.0),
        ] {
            let rotation = nbt(|out| {
                out.extend_from_slice(&[5, 12]);
                out.extend_from_slice(b"ItemRotation");
                out.extend_from_slice(&degrees.to_le_bytes());
            });
            assert_eq!(frame_rotation(&rotation), expected);
        }
        assert_eq!(frame_rotation(&nbt(|_| {})), 0.0);
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
