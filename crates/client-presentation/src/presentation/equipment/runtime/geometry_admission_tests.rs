//! Geometry admission uses the immutable catalog order without per-candidate catalog scans.

use super::*;
use crate::presentation::equipment::tests::held_animation_tests::{owner, owner_rig};
use assets::{EquipmentBinding, EquipmentReference, EquipmentTransform};

const ITEM: &str = "minecraft:test_item";
const TARGET: &str = "geometry.z_missing";

/// Builds sorted geometry payloads and matching owner mappings, optionally supplying their model.
fn parts(
    geometry_count: usize,
    mappings: usize,
    available: bool,
) -> (assets::CompiledEntityAssets, Arc<RuntimeEquipmentCatalog>) {
    let mut compiled = actor_animation::test_support::compiled_fixture();
    let extra = geometry_count - 1;
    compiled.geometries = (0..geometry_count)
        .map(|index| {
            let mut geometry = compiled.geometries[0].clone();
            if index > 0 {
                geometry.identifier = if available && index + 1 == geometry_count {
                    TARGET.into()
                } else {
                    format!("geometry.padding_{index:04}").into()
                };
            }
            geometry
        })
        .collect();
    let mut symbols: Vec<_> = compiled
        .geometries
        .iter()
        .map(|geometry| {
            let mut symbol = compiled.symbols[0].clone();
            symbol.identifier = geometry.identifier.clone();
            symbol
        })
        .collect();
    symbols.extend(compiled.symbols[1..].iter().cloned());
    compiled.animation_clips[0].symbol += extra as u32;
    compiled.rig_bindings[0].entity_symbol += extra as u32;
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += extra as u32;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.molang_symbols = [
        (assets::MolangSymbolKind::Name, "wield"),
        (assets::MolangSymbolKind::Query, "query.is_sneaking"),
    ]
    .into_iter()
    .map(|(kind, identifier)| assets::MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect();
    compiled.molang_expressions = vec![assets::CompiledMolangExpression {
        first_op: 0,
        op_count: 1,
        max_stack: 1,
    }]
    .into();
    compiled.molang_ops = vec![assets::MolangOp::LoadQuery(1)].into();
    let ordinary = EquipmentBinding {
        identifier: ITEM.into(),
        attachable: None,
        category: EquipmentCategory::Held,
        geometry: EquipmentReference {
            identifier: TARGET.into(),
            resolution: if available {
                EntityDependencyResolution::Catalog
            } else {
                EntityDependencyResolution::External
            },
        },
        texture: EquipmentReference {
            identifier: "textures/items/item".into(),
            resolution: EntityDependencyResolution::Catalog,
        },
        resources: Default::default(),
        material: "entity_alphatest".into(),
        color_mask: assets::EquipmentColorMask::NoMask,
        render_controller: "controller.render.item".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::default(),
    };
    let mut bindings = vec![ordinary.clone()];
    let mut rigs = compiled.rig_bindings.into_vec();
    for index in 0..mappings {
        let name: Box<str> = format!("test:candidate_{index:04}").into();
        let mut symbol = symbols[4 + extra].clone();
        symbol.identifier = name.clone();
        let mut rig = rigs[0];
        rig.entity_symbol = symbols.len() as u32;
        rig.first_geometry = compiled.rig_geometries.len() as u32;
        rig.geometry_count = 0;
        rig.render_controller = None;
        rig.item_condition = Some(0);
        symbols.push(symbol);
        rigs.push(rig);
        bindings.push(EquipmentBinding {
            attachable: Some(name),
            ..ordinary.clone()
        });
    }
    compiled.symbols = symbols.into();
    compiled.rig_bindings = rigs.into();
    let catalog =
        Arc::new(RuntimeEquipmentCatalog::from_parts([1; 32], bindings, Vec::new()).unwrap());
    (compiled, catalog)
}

/// Admits independent immutable entity and equipment catalogs without installed carriers.
fn catalogs(
    geometries: usize,
    mappings: usize,
    available: bool,
) -> (Arc<RuntimeEntityAssets>, Arc<RuntimeEquipmentCatalog>) {
    let (compiled, catalog) = parts(geometries, mappings, available);
    (
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap()),
        catalog,
    )
}

/// Builds the real selection owner with an empty artwork catalog, which admission does not read.
fn runtime(
    assets: Arc<RuntimeEntityAssets>,
    catalog: Option<Arc<RuntimeEquipmentCatalog>>,
) -> EquipmentRuntime {
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&assets::encode_icon_catalog([1; 32], &[], &[]).unwrap())
            .unwrap(),
    );
    EquipmentRuntime::build(assets, catalog, icons, None, None, Default::default()).0
}

/// Supplies the complete stack fields used by the production owner-selection entry point.
fn item() -> WornItem {
    WornItem {
        identifier: ITEM.into(),
        kind: HeldKind::Other,
        metadata: 0,
        damage: None,
        variant: Default::default(),
        map_id: None,
        charged_projectile: None,
        use_facts: Default::default(),
        grip: Default::default(),
        dye_rgb: None,
        enchanted: false,
        glint: Default::default(),
    }
}

/// Selects through both production layer callbacks using a live owner snapshot and rig context.
fn select(
    runtime: &mut EquipmentRuntime,
    owner: &client_world::ActorSnapshot,
) -> Option<pack::BindingSource> {
    let names = [Box::from("rightItem")];
    let rig = owner_rig(owner, &names);
    runtime.selected_binding_source(
        &item(),
        EquipmentAnimation {
            owner,
            rig: &rig,
            frame_alpha: 1.0,
            delta_seconds: 0.05,
        },
        Default::default(),
    )
}

#[test]
fn matching_missing_geometry_candidates_bound_actual_lookup_work_in_both_asset_layers() {
    let mut owner = owner();
    owner.metadata.insert(
        protocol_ids::ActorDataKey::Flags.wire_value(),
        protocol::ActorMetadataValue::Flags(1 << protocol_ids::ActorFlag::Sneaking.wire_value()),
    );
    let (assets, catalog) = catalogs(4_097, assets::MAX_EQUIPMENT_BINDINGS - 1, false);
    let per_lookup = 2 * (assets.geometries().len().ilog2() as usize + 2);
    for pack in [false, true] {
        let mut runtime = runtime(Arc::clone(&assets), (!pack).then(|| Arc::clone(&catalog)));
        if pack {
            runtime.set_pack_layer(Some((
                Arc::clone(&assets),
                Some(Arc::clone(&catalog)),
                Vec::new(),
            )));
        }
        for _ in 0..3 {
            RuntimeEntityAssets::take_geometry_lookup_work();
            let source = select(&mut runtime, &owner);
            if pack {
                assert!(
                    source.is_none(),
                    "pack external geometry alone is not drawable"
                );
            } else {
                let source = source.unwrap();
                assert!(!source.from_pack);
                assert!(
                    source.catalog.bindings()[source.index].attachable.is_none(),
                    "ordinary external startup fallback remains admitted"
                );
            }
            let (calls, comparisons) = RuntimeEntityAssets::take_geometry_lookup_work();
            assert_eq!(calls, catalog.bindings().len() * if pack { 2 } else { 1 });
            assert!(comparisons >= calls);
            assert!(
                comparisons <= calls * per_lookup,
                "{comparisons} comparisons exceeded the immutable lookup bound"
            );
        }
    }

    let mut predicates = actor_animation::AttachablesRuntime::new(Arc::clone(&assets));
    let names = [Box::from("rightItem")];
    let rig = owner_rig(&owner, &names);
    for limit in [
        actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK,
        actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK,
        1,
        0,
    ] {
        RuntimeEntityAssets::take_geometry_lookup_work();
        let mut left = limit;
        let selected = predicates.select_equipment_binding(
            &catalog,
            ITEM,
            &owner,
            &rig,
            Default::default(),
            &mut left,
            |binding| {
                !assets
                    .geometry_candidates(&binding.geometry.identifier)
                    .is_empty()
            },
        );
        assert_eq!(selected, None);
        let (calls, comparisons) = RuntimeEntityAssets::take_geometry_lookup_work();
        assert_eq!(
            calls,
            if limit > 1 {
                catalog.bindings().len()
            } else {
                1
            }
        );
        assert!(comparisons <= calls * per_lookup);
        assert!(left <= limit);
    }
}

#[test]
fn geometry_membership_replacement_preserves_layer_precedence_and_external_fallback() {
    let mut owner = owner();
    owner.metadata.insert(
        protocol_ids::ActorDataKey::Flags.wire_value(),
        protocol::ActorMetadataValue::Flags(1 << protocol_ids::ActorFlag::Sneaking.wire_value()),
    );
    for startup_available in [false, true] {
        let (startup, base) = catalogs(2, 0, startup_available);
        let mut runtime = runtime(Arc::clone(&startup), Some(base));
        for pack_available in [false, true, false, true] {
            let (pack, catalog) = catalogs(3, 1, pack_available);
            runtime.set_pack_layer(Some((Arc::clone(&pack), Some(catalog), Vec::new())));
            let source = select(&mut runtime, &owner).unwrap();
            assert_eq!(source.from_pack, startup_available || pack_available);
            assert_eq!(
                source.catalog.bindings()[source.index].attachable.is_some(),
                startup_available || pack_available
            );
            let geometry = runtime.pack_armor_geometry_for(TARGET);
            if pack_available {
                assert_eq!(
                    geometry.unwrap().rig,
                    render_model::pack_equipment_rig_id(
                        render_model::find_geometry_index(&pack, TARGET).unwrap()
                    )
                );
            } else if startup_available {
                assert_eq!(
                    geometry.unwrap().rig,
                    render_model::equipment_rig_id(
                        render_model::find_geometry_index(&startup, TARGET).unwrap()
                    )
                );
            } else {
                assert!(geometry.is_none());
            }
        }
        runtime.set_pack_layer(None);
        assert!(!select(&mut runtime, &owner).unwrap().from_pack);
    }
    let (assets, catalog) = catalogs(2, 1, false);
    let mapped_only = Arc::new(
        RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            catalog
                .bindings()
                .iter()
                .filter(|binding| binding.attachable.is_some())
                .cloned()
                .collect(),
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(
        select(&mut runtime(assets, Some(mapped_only)), &owner).is_none(),
        "mapped external startup geometry is not an ordinary fallback"
    );
}

#[test]
fn geometry_membership_is_exact_and_preserves_duplicate_source_order() {
    let (mut compiled, _) = parts(2, 0, false);
    let mut sources = compiled.sources.into_vec();
    let mut source = sources[2].clone();
    source.path = "models/entity/z_item.json".into();
    sources.insert(3, source);
    compiled.sources = sources.into();
    for symbol in &mut compiled.symbols {
        if symbol.source_index >= 3 {
            symbol.source_index += 1;
        }
    }
    for candidate in &mut compiled.render.candidates {
        if candidate.source >= 3 {
            candidate.source += 1;
        }
    }
    let mut geometries = compiled.geometries.into_vec();
    let mut duplicate = geometries[0].clone();
    duplicate.source_index = 3;
    geometries.insert(1, duplicate);
    compiled.geometries = geometries.into();
    let mut symbols = compiled.symbols.into_vec();
    let mut duplicate = symbols[0].clone();
    duplicate.source_index = 3;
    symbols.insert(1, duplicate);
    compiled.symbols = symbols.into();
    compiled.animation_clips[0].symbol += 1;
    compiled.rig_bindings[0].entity_symbol += 1;
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    let assets =
        RuntimeEntityAssets::decode(&assets::entity::encode_entity_blob(&compiled).unwrap())
            .unwrap();
    let matching = assets.geometry_candidates("geometry.item");
    assert_eq!(
        matching
            .iter()
            .map(|geometry| geometry.source_index)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(
        render_model::find_geometry_index(&assets, "geometry.item"),
        Some(0)
    );
    for name in [
        "Geometry.item",
        "geometry.Item",
        "geometry.item.extra",
        "",
        TARGET,
    ] {
        assert!(
            assets.geometry_candidates(name).is_empty(),
            "lookup must preserve exact identifier semantics: {name}"
        );
    }
}
