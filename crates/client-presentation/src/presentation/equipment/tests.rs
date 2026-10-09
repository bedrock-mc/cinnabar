#[path = "runtime/elytra_tests.rs"]
mod elytra_tests;

#[path = "runtime/held_animation_tests.rs"]
mod held_animation_tests;

use std::sync::Arc;

use assets::IconSprite;
use bevy::math::{Quat, Vec3};
use render::{
    ACTOR_LAYER_BODY, ActorArtworkPages, ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, EquipmentRaster,
};
use render_model::{EntityRigId, RenderBoneTransform};

use super::{
    armor::{bone_map, hidden_bone, pack_tint, remap_pose},
    atlas::{ATLAS_SIDE, SpriteAtlas},
    display::{
        FirstPersonHand, FirstPersonShape, ItemDisplay, attach_to_bone, first_person_display,
        held_block_display, is_rod,
    },
    runtime::{FirstPersonArms, layer_presentation},
};

fn sprite(side: u16, fill: u8) -> IconSprite {
    IconSprite {
        width: side,
        height: side,
        rgba8: vec![fill; usize::from(side) * usize::from(side) * 4].into(),
    }
}

fn bone(translation: [f32; 3], scale: f32) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [translation[0], translation[1], translation[2], scale],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    }
}

#[test]
fn atlas_places_sprites_without_overlap_and_copies_their_pixels() {
    let sprites = [sprite(16, 1), sprite(32, 2), sprite(16, 3)];
    let atlas = SpriteAtlas::pack(&sprites);
    assert_eq!(atlas.layers.len(), 1);
    let placements = atlas
        .placements
        .iter()
        .map(|placement| placement.expect("every sprite fits"))
        .collect::<Vec<_>>();
    for (index, placement) in placements.iter().enumerate() {
        let offset =
            (usize::from(placement.y) * usize::from(ATLAS_SIDE) + usize::from(placement.x)) * 4;
        assert_eq!(
            atlas.layers[placement.layer].rgba8[offset],
            sprites[index].rgba8[0]
        );
        for other in &placements[..index] {
            let disjoint = placement.x + placement.width <= other.x
                || other.x + other.width <= placement.x
                || placement.y + placement.height <= other.y
                || other.y + other.height <= placement.y;
            assert!(disjoint);
        }
    }
    let rect = placements[1].uv_rect();
    assert!(rect[0] >= 0.0 && rect[2] <= 1.0 && rect[2] > rect[0] && rect[3] > rect[1]);
}

#[test]
fn atlas_spills_into_extra_layers_and_skips_invalid_sprites() {
    let size = ATLAS_SIDE.min(32);
    let count = usize::from(ATLAS_SIDE).pow(2) / usize::from(size).pow(2) + 1;
    let mut sprites = (0..count).map(|_| sprite(size, 9)).collect::<Vec<_>>();
    sprites.push(IconSprite {
        width: 4,
        height: 4,
        rgba8: Arc::from([0u8; 3]),
    });
    let atlas = SpriteAtlas::pack(&sprites);
    assert!(
        atlas.layers.len() > 1,
        "sprites exceed a single atlas layer"
    );
    assert!(atlas.placements[..count].iter().all(Option::is_some));
    assert!(atlas.placements[count].is_none());
}

#[test]
fn armor_bones_follow_same_named_body_bones_and_hide_when_unmatched() {
    let names = |list: &[&str]| {
        list.iter()
            .map(|name| Box::<str>::from(*name))
            .collect::<Vec<_>>()
    };
    let body_names = names(&["root", "body", "head", "rightArm"]);
    let armor_names = names(&["body", "HEAD", "rightItem"]);
    let map = bone_map(&armor_names, &body_names);
    assert_eq!(map, vec![Some(1), Some(2), None]);
    let body = [
        bone([0.0; 3], 1.0),
        bone([1.0; 3], 1.0),
        bone([2.0; 3], 1.0),
        bone([3.0; 3], 1.0),
    ];
    let pose = remap_pose(&map, &body);
    assert_eq!(pose[0], body[1]);
    assert_eq!(pose[1], body[2]);
    assert_eq!(pose[2], hidden_bone());
}

#[test]
fn tint_packs_rgb_into_abgr_with_the_enabled_alpha() {
    assert_eq!(pack_tint(0x0011_2233), 0xff33_2211);
    assert_ne!(pack_tint(0), 0);
}

#[test]
fn equipment_layer_shares_the_body_identity_transform_and_generations() {
    let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(&[EquipmentRaster {
        width: 2,
        height: 2,
        rgba8: vec![255; 16].into(),
    }]);
    let location = locations[0].unwrap();
    let body = ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 3,
                ingress_sequence: 4,
                source_tick: None,
                movement_revision: 5,
                pose_generation: 6,
                layer: ACTOR_LAYER_BODY,
            },
            rig: EntityRigId(0),
            previous_bones: Arc::from([bone([0.0; 3], 1.0)]),
            current_bones: Arc::from([bone([0.0; 3], 1.0)]),
            completed_tick: 7,
            reset_generation: 8,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 9.0],
            [0.0, 1.0, 0.0, 10.0],
            [0.0, 0.0, 1.0, 11.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0x6600_00ff,
    };
    let layer = layer_presentation(
        &body,
        4,
        EntityRigId(0x8000_0001),
        [
            Arc::from([bone([0.0; 3], 1.0)]),
            Arc::from([bone([1.0; 3], 1.0)]),
        ],
        location,
        0xff00_00ff,
    );
    let submission = layer.submission;
    assert_eq!(submission.input.identity.layer, 4);
    assert_eq!(submission.input.identity.runtime_id, 2);
    assert_eq!(submission.input.completed_tick, 7);
    assert_eq!(submission.input.reset_generation, 8);
    assert_eq!(submission.world_from_actor, body.world_from_actor);
    assert_eq!(submission.texture_layer, location.layer());
    assert_eq!(submission.tint, 0xff00_00ff);
    assert_eq!(submission.overlay_rgba8, 0x6600_00ff);
}

#[test]
fn first_person_arms_follow_the_render_controller_visibility() {
    let arms = |main, off| FirstPersonArms::for_hands(main, off);
    assert_eq!(
        arms(None, None),
        FirstPersonArms {
            right: true,
            left: false
        }
    );
    assert_eq!(
        arms(Some("minecraft:diamond_sword"), None),
        FirstPersonArms {
            right: false,
            left: false
        }
    );
    assert_eq!(
        arms(Some("minecraft:filled_map"), None),
        FirstPersonArms {
            right: true,
            left: true
        }
    );
    assert!(!arms(Some("minecraft:filled_map"), Some("minecraft:shield")).left);
    assert!(arms(None, Some("minecraft:filled_map")).left);
}

/// A held item with no drawable layer keeps the bare arm, so the first-person rig still swings.
#[test]
fn undrawn_main_hand_item_keeps_the_swinging_arm() {
    let held = FirstPersonArms::for_hands(Some("zeqa:item.ffa"), None);
    assert!(!held.with_undrawn_main(true).right);
    assert!(held.with_undrawn_main(false).right);
    assert!(!held.with_undrawn_main(false).left);
    let empty = FirstPersonArms::for_hands(None, None);
    assert_eq!(empty.with_undrawn_main(false), empty);
}

#[test]
fn head_items_map_to_their_skull_kinds_and_others_to_none() {
    use render::SkullKind;
    let kind = super::runtime::skull_kind;
    assert_eq!(kind("minecraft:zombie_head"), Some(SkullKind::Zombie));
    assert_eq!(kind("minecraft:skeleton_skull"), Some(SkullKind::Skeleton));
    assert_eq!(kind("minecraft:player_head"), Some(SkullKind::Player));
    assert_eq!(kind("minecraft:dragon_head"), None);
    assert_eq!(kind("minecraft:carved_pumpkin"), None);
}

/// Reads an installed equipment fixture, reporting a missing file without hiding other errors.
fn local_carrier(name: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled")
        .join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping local equipment fixture test: missing {}; make assets",
                path.display()
            );
            None
        }
        Err(error) => panic!("read equipment fixture {}: {error}", path.display()),
    }
}

// Local-only: which held items and armor pieces the real carriers can draw on a player body.
#[test]
fn real_carriers_draw_armor_and_report_each_held_item() {
    use super::runtime::{ActorEquipmentInput, EquipmentRuntime, HeldKind, WornItem};
    let (Some(entities), Some(icons), Some(equipment)) = (
        local_carrier("vanilla-v1.mcbeent"),
        local_carrier("vanilla-v1.mcbeico"),
        local_carrier("vanilla-v1.mcbeeqp"),
    ) else {
        return;
    };
    let entities = assets::RuntimeEntityAssets::decode(&entities).unwrap();
    let icons = assets::RuntimeIconCatalog::decode(&icons).unwrap();
    let catalog = assets::RuntimeEquipmentCatalog::decode(&equipment).unwrap();
    let (entities, icons, catalog) = (Arc::new(entities), Arc::new(icons), Arc::new(catalog));
    let (mut runtime, _, _) = EquipmentRuntime::build(
        entities,
        Some(catalog),
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let names = [
        "root",
        "body",
        "waist",
        "head",
        "hat",
        "rightArm",
        "leftArm",
        "rightLeg",
        "leftLeg",
        "rightItem",
        "leftItem",
    ]
    .map(Box::<str>::from)
    .to_vec();
    let rig = EntityRigId(0x7000_0000);
    runtime.register_skin_rig(rig, names.clone());
    let pose: Arc<[RenderBoneTransform]> = names.iter().map(|_| bone([0.0; 3], 1.0)).collect();
    let body = ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: 0,
                layer: ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 0,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let worn = |identifier: &str| WornItem {
        identifier: Arc::from(identifier),
        metadata: 0,
        damage: None,
        kind: HeldKind::Sprite,
        dye_rgb: None,
        enchanted: false,
    };
    let armor = ActorEquipmentInput {
        armor: [
            "minecraft:diamond_helmet",
            "minecraft:diamond_chestplate",
            "minecraft:diamond_leggings",
            "minecraft:diamond_boots",
        ]
        .map(|identifier| Some(worn(identifier))),
        ..ActorEquipmentInput::default()
    };
    assert_eq!(runtime.layers_for(&body, &armor, None).len(), 4);
    let held = [
        "minecraft:ender_pearl",
        "minecraft:diamond_sword",
        "minecraft:golden_apple",
    ]
    .map(|identifier| {
        let input = ActorEquipmentInput {
            main: Some(worn(identifier)),
            ..ActorEquipmentInput::default()
        };
        (identifier, runtime.layers_for(&body, &input, None).len())
    });
    eprintln!("{held:?}");
    assert_eq!(held[0].1, 1);
}

fn png(side: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(side, side, image::Rgba([200, 40, 40, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A player-shaped body submission on a registered skin rig.
fn player_body(runtime: &mut super::runtime::EquipmentRuntime) -> ActorRigSubmission {
    let names = [
        "root",
        "body",
        "head",
        "rightArm",
        "leftArm",
        "rightItem",
        "leftItem",
    ]
    .map(Box::<str>::from)
    .to_vec();
    let rig = EntityRigId(0x7000_0000);
    runtime.register_skin_rig(rig, names.clone());
    let pose: Arc<[RenderBoneTransform]> = names.iter().map(|_| bone([0.0; 3], 1.0)).collect();
    ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: 0,
                layer: ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 0,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}

/// An equipment runtime over a server pack's attachables, with no startup carriers.
fn pack_runtime(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> (super::runtime::EquipmentRuntime, ActorArtworkPages) {
    use super::runtime::EquipmentRuntime;
    let compiled = pack_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("pack compiles");
    let catalog = Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            compiled.identity,
            compiled.equipment_bindings,
            compiled.equipment_textures,
        )
        .unwrap(),
    );
    let entities = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&entities),
        None,
        icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    let (pages, locations) =
        pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, catalog, locations)));
    (runtime, pages)
}

fn session_items(
    components: Vec<(&str, protocol::ItemComponents)>,
    icons: Vec<&str>,
) -> crate::session_assets::SessionItems {
    use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
    crate::session_assets::SessionItems {
        components: Arc::new(
            components
                .into_iter()
                .map(|(identifier, components)| (Arc::from(identifier), components))
                .collect(),
        ),
        icons: (!icons.is_empty()).then(|| {
            Arc::new(SessionIcons {
                icons: icons
                    .into_iter()
                    .map(|identifier| SessionIcon {
                        identifier: identifier.into(),
                        metadata: 0,
                        width: 16,
                        height: 16,
                        rgba8: vec![255; 16 * 16 * 4].into(),
                    })
                    .collect(),
                ..Default::default()
            })
        }),
    }
}

fn crown_pack() -> Vec<(Box<str>, Vec<u8>)> {
    vec![
        (
            "attachables/crown.json".into(),
            br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":"test:crown","materials":{"default":"armor"},"textures":{"default":"textures/models/crown"},"geometry":{"default":"geometry.test.crown"},"render_controllers":["controller.render.armor"]}}}"#.to_vec(),
        ),
        (
            "models/entity/crown.geo.json".into(),
            br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test.crown","texture_width":16,"texture_height":16},"bones":[{"name":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,32,-4],"size":[8,2,8],"uv":[0,0]}]}]}]}"#.to_vec(),
        ),
        ("textures/models/crown.png".into(), png(16)),
    ]
}

#[test]
fn java_hand_preserves_a_pack_bow_binding_selected_ahead_of_vanilla() {
    let files = crown_pack()
        .into_iter()
        .map(|(path, bytes)| {
            let bytes = if path.ends_with(".json") {
                String::from_utf8(bytes)
                    .unwrap()
                    .replace("test:crown", "minecraft:bow")
                    .into_bytes()
            } else {
                bytes
            };
            (path, bytes)
        })
        .collect();
    let (mut runtime, _) = pack_runtime(files);
    assert!(runtime.is_vanilla_attachable("minecraft:bow"));
    runtime.set_pack_layer(None);
    assert!(!runtime.is_vanilla_attachable("minecraft:bow"));
}

// A custom attachable whose name and geometry say nothing is worn where `minecraft:wearable` puts it.
#[test]
fn wearable_slot_places_an_unnamed_custom_attachable_on_the_body() {
    use super::runtime::{ActorEquipmentInput, HeldKind, WornItem};
    let (mut runtime, _) = pack_runtime(crown_pack());
    let body = player_body(&mut runtime);
    let crown = WornItem {
        identifier: Arc::from("test:crown"),
        metadata: 0,
        damage: None,
        kind: HeldKind::Other,
        dye_rgb: None,
        enchanted: false,
    };
    let worn = ActorEquipmentInput {
        armor: [Some(crown), None, None, None],
        ..ActorEquipmentInput::default()
    };
    assert!(runtime.layers_for(&body, &worn, None).is_empty());
    let components = protocol::ItemComponents {
        wearable_slot: Some("slot.armor.head".into()),
        ..Default::default()
    };
    let items = session_items(vec![("test:crown", components)], vec![]);
    runtime.set_session_items(Some(&items), None, Vec::new());
    assert_eq!(runtime.layers_for(&body, &worn, None).len(), 1);
    runtime.set_session_items(None, None, Vec::new());
    assert!(runtime.layers_for(&body, &worn, None).is_empty());
}

#[test]
fn first_person_session_icon_is_independent_of_avatar_bones_and_keeps_its_atlas() {
    use super::runtime::{ActorEquipmentInput, HeldKind, StagedSessionIcons, WornItem};
    let (mut runtime, pages) = pack_runtime(crown_pack());
    let body = player_body(&mut runtime);
    let item = WornItem {
        identifier: Arc::from("test:gem"),
        metadata: 0,
        damage: None,
        kind: HeldKind::Other,
        dye_rgb: None,
        enchanted: false,
    };
    let items = session_items(vec![("test:gem", Default::default())], vec!["test:gem"]);
    let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
    let (_, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(Some(&items), Some(staged), locations);
    let animation = client_world::ItemAnimationState::default();
    let first = runtime.first_person_item(&body, &item, animation).unwrap();
    assert!(first.camera_space);
    let mut moved = body.clone();
    let changed = body
        .input
        .current_bones
        .iter()
        .map(|_| {
            let mut bone = bone([20.0, -12.0, 8.0], 3.0);
            bone.rotation = Quat::from_rotation_x(1.7).to_array();
            bone
        })
        .collect::<Vec<_>>();
    moved.input.previous_bones = Arc::from(changed.clone());
    moved.input.current_bones = Arc::from(changed);
    let second = runtime.first_person_item(&moved, &item, animation).unwrap();
    assert_eq!(
        first.presentation.submission.input.previous_bones,
        second.presentation.submission.input.previous_bones
    );
    assert_eq!(
        first.presentation.submission.input.current_bones,
        second.presentation.submission.input.current_bones
    );
    assert_eq!(first.presentation.location, second.presentation.location);
    let third = runtime.layers_for(
        &body,
        &ActorEquipmentInput {
            main: Some(item),
            ..Default::default()
        },
        None,
    );
    assert_eq!(third[0].location, first.presentation.location);
    assert_ne!(
        third[0].submission.input.current_bones,
        first.presentation.submission.input.current_bones
    );
}

// A custom item with no attachable holds its pack icon, gripped per `hand_equipped`.
#[test]
fn custom_items_hold_their_session_icon_with_the_component_grip() {
    use super::runtime::{ActorEquipmentInput, HeldKind, StagedSessionIcons, WornItem};
    let (mut runtime, pages) = pack_runtime(crown_pack());
    let body = player_body(&mut runtime);
    let held = |identifier: &str| ActorEquipmentInput {
        main: Some(WornItem {
            identifier: Arc::from(identifier),
            metadata: 0,
            damage: None,
            kind: HeldKind::Other,
            dye_rgb: None,
            enchanted: false,
        }),
        ..ActorEquipmentInput::default()
    };
    assert!(
        runtime
            .layers_for(&body, &held("test:blade"), None)
            .is_empty()
    );
    let blade = protocol::ItemComponents {
        hand_equipped: true,
        use_duration_ticks: Some(24),
        ..Default::default()
    };
    let items = session_items(
        vec![("test:blade", blade), ("test:gem", Default::default())],
        vec!["test:blade", "test:gem"],
    );
    let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
    let (_, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(Some(&items), Some(staged), locations);
    let blade_layers = runtime
        .layers_for(&body, &held("test:blade"), None)
        .to_vec();
    let gem_layers = runtime.layers_for(&body, &held("test:gem"), None);
    assert_eq!((blade_layers.len(), gem_layers.len()), (1, 1));
    let bone = |layers: &[super::runtime::EquipmentPresentation]| {
        layers[0].submission.input.current_bones[0]
    };
    // Upright and flat grips place the icon differently.
    assert_ne!(bone(&blade_layers).rotation, bone(&gem_layers).rotation);
    assert_eq!(
        runtime.item_use_durations().get("test:blade").copied(),
        Some(24)
    );
    assert!(!runtime.take_pending_geometries().is_empty());
}

// A custom block item whose session icons carry a cube sheet is held as that cube in vanilla's
// first-person and third-person block poses, not as its flat thumbnail.
#[test]
fn custom_block_items_with_a_cube_sheet_are_held_as_blocks() {
    use super::runtime::{ActorEquipmentInput, HeldKind, StagedSessionIcons, WornItem};
    use client_ui::ui_runtime::presentation::SessionIcon;
    let (mut runtime, pages) = pack_runtime(crown_pack());
    let body = player_body(&mut runtime);
    let item = WornItem {
        identifier: Arc::from("test:controller"),
        metadata: 0,
        damage: None,
        kind: HeldKind::Other,
        dye_rgb: None,
        enchanted: false,
    };
    let mut items = session_items(
        vec![("test:controller", Default::default())],
        vec!["test:controller"],
    );
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    Arc::get_mut(items.icons.as_mut().unwrap())
        .unwrap()
        .block_sheets
        .push(SessionIcon {
            identifier: Arc::from("test:controller"),
            metadata: 0,
            width,
            height,
            rgba8: vec![255; (width * height * 4) as usize].into(),
        });
    let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
    let (_, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(Some(&items), Some(staged), locations);
    let animation = client_world::ItemAnimationState::default();
    let first = runtime.first_person_item(&body, &item, animation).unwrap();
    let expected = super::first_person::block_pose(animation).unwrap();
    assert_eq!(
        &*first.presentation.submission.input.current_bones,
        &[expected]
    );
    let third = runtime.layers_for(
        &body,
        &ActorEquipmentInput {
            main: Some(item),
            ..Default::default()
        },
        None,
    );
    let right_item = body.input.current_bones[5];
    let grip = attach_to_bone(right_item, held_block_display()).unwrap();
    assert_eq!(&*third[0].submission.input.current_bones, &[grip]);
}

const REST: FirstPersonHand = FirstPersonHand {
    swing: 0.0,
    equip: 1.0,
    consume: None,
};

/// Camera-space position of texel corner `(column, row)` of a 16-texel held sprite.
fn icon_point(display: ItemDisplay, column: f32, row: f32) -> Vec3 {
    let local = Vec3::new(-column / 16.0, 1.0 - row / 16.0, 0.0);
    display.translation + display.rotation * (local * display.scale)
}

// First person draws the item in camera space, as vanilla's recorded first-person sword: the
// handle low on the right and the blade rising up and to the right, face towards the camera.
#[test]
fn first_person_sprite_rises_up_right_facing_the_camera() {
    let display = first_person_display(
        FirstPersonShape::Sprite {
            mirrored_art: false,
        },
        REST,
    );
    let handle = icon_point(display, 1.0, 15.0);
    let tip = icon_point(display, 15.0, 1.0);
    assert!(
        handle.x > 0.0 && handle.y < 0.0 && handle.z < 0.0,
        "{handle}"
    );
    let screen = |point: Vec3| Vec3::new(point.x / -point.z, point.y / -point.z, 0.0);
    assert!(screen(tip).x > screen(handle).x && screen(tip).y > screen(handle).y);
    let normal = display.rotation * Vec3::Z;
    let centre = icon_point(display, 8.0, 8.0);
    assert!(
        normal.dot(centre).abs() > 0.5 * centre.length(),
        "faces the camera"
    );
    assert!(
        (display.scale - 0.6).abs() < 1e-5,
        "0.4 hand scale over the 1.5 default"
    );
}

// The equip dip lowers the item 0.6 blocks while the new item is taken, and a rod turns about.
#[test]
fn first_person_equip_dip_and_mirrored_art() {
    let sprite = FirstPersonShape::Sprite {
        mirrored_art: false,
    };
    let rest = first_person_display(sprite, REST);
    let dipped = first_person_display(sprite, FirstPersonHand { equip: 0.0, ..REST });
    assert!((rest.translation.y - dipped.translation.y - 0.6).abs() < 1e-5);
    let turned = first_person_display(FirstPersonShape::Sprite { mirrored_art: true }, REST);
    let half_turn = rest.rotation.inverse() * turned.rotation;
    assert!(half_turn.angle_between(Quat::IDENTITY) > 3.0);
    assert!(is_rod("minecraft:fishing_rod") && !is_rod("minecraft:stick"));
    let block = first_person_display(FirstPersonShape::Block, REST);
    assert!((block.scale - 0.4).abs() < 1e-5);
    assert!(
        (block.translation - Vec3::new(0.56, -0.52, -0.72)).length() < 1e-5,
        "the cube centres on the hand anchor"
    );
}

// Eating raises the item towards the mouth once the first fifth of the use has passed.
#[test]
fn first_person_consume_raises_the_item() {
    let sprite = FirstPersonShape::Sprite {
        mirrored_art: false,
    };
    let rest = first_person_display(sprite, REST);
    let eating = first_person_display(
        sprite,
        FirstPersonHand {
            consume: Some((16.0, 32.0)),
            ..REST
        },
    );
    assert!(eating.translation.distance(rest.translation) > 0.3);
    let started = first_person_display(
        sprite,
        FirstPersonHand {
            consume: Some((0.0, 32.0)),
            ..REST
        },
    );
    assert!(started.translation.distance(rest.translation) < 0.05);
}

/// Java grips ride the right arm, not the hand bone vanilla still turns, and skip the hurt flash.
#[test]
fn java_grips_ride_the_arm_and_skip_the_hurt_flash() {
    use super::runtime::{ActorEquipmentInput, HeldKind, JavaGrip, StagedSessionIcons, WornItem};
    use render_model::java_animation::{JavaHeldItem, JavaItemMesh, third_person_item};
    let (mut runtime, pages) = pack_runtime(crown_pack());
    let mut body = player_body(&mut runtime);
    let items = session_items(vec![("test:gem", Default::default())], vec!["test:gem"]);
    let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
    let (_, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(Some(&items), Some(staged), locations);
    let turned = |rotation: Quat, translation: [f32; 3]| RenderBoneTransform {
        rotation: rotation.to_array(),
        ..bone(translation, 1.0)
    };
    let arm = turned(Quat::from_rotation_x(0.7), [0.3, 1.4, 0.0]);
    let mut pose = body.input.current_bones.to_vec();
    pose[3] = arm;
    pose[5] = turned(Quat::from_rotation_z(1.1), [0.4, 0.9, 0.1]);
    body.input.previous_bones = pose.clone().into();
    body.input.current_bones = pose.into();
    body.overlay_rgba8 = 0x6600_00ff;
    let input = ActorEquipmentInput {
        main: Some(WornItem {
            identifier: Arc::from("test:gem"),
            metadata: 0,
            damage: None,
            kind: HeldKind::Other,
            dye_rgb: None,
            enchanted: false,
        }),
        java: Some(JavaGrip::default()),
        ..ActorEquipmentInput::default()
    };
    let layers = runtime.layers_for(&body, &input, None);
    assert_eq!(layers.len(), 1);
    let expected = attach_to_bone(
        arm,
        ItemDisplay::from_matrix(third_person_item(JavaHeldItem::Flat, JavaItemMesh::Sprite)),
    )
    .unwrap();
    let held = layers[0].submission.input.current_bones[0];
    for (actual, expected) in held
        .translation_scale
        .iter()
        .chain(&held.rotation)
        .zip(expected.translation_scale.iter().chain(&expected.rotation))
    {
        assert!((actual - expected).abs() < 1e-5, "{held:?} vs {expected:?}");
    }
    assert_eq!(layers[0].submission.overlay_rgba8, 0);
}
