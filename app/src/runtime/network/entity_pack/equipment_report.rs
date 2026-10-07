//! Env-gated check over locally cached server packs: custom armor attachables draw on a body.

use std::sync::Arc;

use render::{
    ACTOR_LAYER_BODY, ActorArtworkPages, ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission,
};
use render_model::{EntityRigId, RenderBoneTransform};

use crate::presentation::equipment::{ActorEquipmentInput, EquipmentRuntime, HeldKind, WornItem};

fn body(runtime: &mut EquipmentRuntime) -> ActorRigSubmission {
    let names = [
        "root", "body", "head", "rightArm", "leftArm", "rightLeg", "leftLeg",
    ]
    .map(Box::<str>::from)
    .to_vec();
    let rig = EntityRigId(0x7000_0000);
    runtime.register_skin_rig(rig, names.clone());
    let rest = RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let pose: Arc<[RenderBoneTransform]> = names.iter().map(|_| rest).collect();
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

/// Env-gated: `CINNABAR_PACKCACHE_DIR` holds cached pack zips and `CINNABAR_CARRIER_DIR` the
/// compiled vanilla carriers; custom armor attachables with compiled textures, given the
/// `wearable` slot their binding declares, draw on a player body.
#[test]
fn cached_pack_custom_armor_draws_in_its_wearable_slot() {
    let (Some(packs), Some(carriers)) = (
        std::env::var_os("CINNABAR_PACKCACHE_DIR"),
        std::env::var_os("CINNABAR_CARRIER_DIR"),
    ) else {
        eprintln!(
            "skipping cached_pack_custom_armor_draws_in_its_wearable_slot: fixture unavailable; requires offline cached custom-armor packs and compiled carriers"
        );
        return;
    };
    let carriers = std::path::PathBuf::from(carriers);
    let read = |name: &str| std::fs::read(carriers.join(name)).unwrap();
    let entities =
        Arc::new(assets::RuntimeEntityAssets::decode(&read("vanilla-v1.mcbeent")).unwrap());
    let icons = Arc::new(assets::RuntimeIconCatalog::decode(&read("vanilla-v1.mcbeico")).unwrap());
    let refs = assets::VanillaEntityRefs::from_json(&read("vanilla-v1.vanillarefs.json"));
    let mut zips = std::fs::read_dir(packs)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .collect::<Vec<_>>();
    zips.sort();
    let (mut checked, mut drawn) = (0usize, 0usize);
    for path in zips {
        let Some(view) = super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let files = super::collect::collect_files(&view, refs.as_ref(), None);
        let Ok(Some(compiled)) = pack_compiler::compile_actor_pack(files) else {
            continue;
        };
        let custom_armor = compiled
            .equipment_bindings
            .iter()
            .filter(|binding| !binding.identifier.starts_with("minecraft:"))
            .filter_map(|binding| match binding.category {
                assets::EquipmentCategory::Armor { slot } => {
                    Some((binding.identifier.clone(), slot))
                }
                _ => None,
            })
            .take(8)
            .collect::<Vec<_>>();
        // Textures past the pack's equipment texture budget leave their attachable undrawn.
        let textured = |item: &str| {
            compiled.equipment_bindings.iter().any(|binding| {
                binding.identifier.as_ref() == item
                    && compiled
                        .equipment_textures
                        .iter()
                        .any(|texture| texture.identifier == binding.texture.identifier)
            })
        };
        let custom_armor = custom_armor
            .into_iter()
            .filter(|(item, _)| textured(item))
            .collect::<Vec<_>>();
        if custom_armor.is_empty() {
            continue;
        }
        let Ok(catalog) = assets::RuntimeEquipmentCatalog::from_parts(
            compiled.identity,
            compiled.equipment_bindings,
            compiled.equipment_textures,
        ) else {
            continue;
        };
        let catalog = Arc::new(catalog);
        let pack_assets =
            Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
        let (mut runtime, pages, _) = EquipmentRuntime::build(
            Arc::clone(&entities),
            None,
            Arc::clone(&icons),
            None,
            None,
            ActorArtworkPages::default(),
        );
        let (_, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((pack_assets, catalog, locations)));
        let body = body(&mut runtime);
        for (item, slot) in custom_armor {
            let slot_name = match slot {
                assets::ArmorSlot::Helmet => "slot.armor.head",
                assets::ArmorSlot::Chestplate => "slot.armor.chest",
                assets::ArmorSlot::Leggings => "slot.armor.legs",
                assets::ArmorSlot::Boots => "slot.armor.feet",
            };
            let items = super::SessionItems {
                components: Arc::new(
                    [(
                        Arc::<str>::from(item.as_ref()),
                        protocol::ItemComponents {
                            wearable_slot: Some(slot_name.into()),
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                ),
                icons: None,
            };
            runtime.set_session_items(Some(&items), None, Vec::new());
            let mut input = ActorEquipmentInput::default();
            input.armor[slot as usize] = Some(WornItem {
                identifier: Arc::from(item.as_ref()),
                metadata: 0,
                damage: None,
                kind: HeldKind::Other,
                dye_rgb: None,
                enchanted: false,
            });
            checked += 1;
            let ok = runtime.layers_for(&body, &input, None).len() == 1;
            if !ok {
                eprintln!("undrawn: {} {item} {slot:?}", path.display());
            }
            drawn += usize::from(ok);
        }
    }
    eprintln!("custom armor drawn: {drawn}/{checked}");
    // Artwork page budgets can still drop a texture; most must draw.
    assert!(checked > 0, "fixture must contain textured custom armor");
    assert!(drawn * 4 >= checked * 3, "{drawn}/{checked}");
}
