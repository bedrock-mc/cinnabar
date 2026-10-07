//! Env-gated report over locally cached server packs: entity compile outcomes.

#[test]
fn captured_entity_stack_retains_animation_payloads() {
    let Some(dir) = std::env::var_os("CINNABAR_ENTITY_STACK_FIXTURE") else {
        eprintln!(
            "skipping captured_entity_stack_retains_animation_payloads: fixture unavailable; requires CINNABAR_ENTITY_STACK_FIXTURE containing stack.json and offline cached packs"
        );
        return;
    };
    #[derive(serde::Deserialize)]
    struct StackEntry {
        file: String,
        subpack: String,
    }
    let dir = std::path::PathBuf::from(dir);
    let entries: Vec<StackEntry> =
        serde_json::from_slice(&std::fs::read(dir.join("stack.json")).unwrap()).unwrap();
    let expected_packs = entries.len();
    let archives = entries
        .into_iter()
        .map(|entry| {
            let path = dir.join(entry.file);
            let stem = path.file_stem().unwrap().to_string_lossy();
            let (id, version) = stem.split_once('_').expect("<uuid>_<version> file name");
            protocol::ResourcePackArchive::with_content_key(
                id.parse().unwrap(),
                version.into(),
                entry.subpack,
                std::fs::read(&path).unwrap(),
                std::fs::read(path.with_extension("key")).unwrap_or_default(),
            )
        })
        .collect();
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(archives));
    assert!(stack.rejections().is_empty(), "offline stack was rejected");
    assert_eq!(stack.packs().len(), expected_packs);
    let Some(refs) = local_vanilla_refs() else {
        eprintln!(
            "skipping captured_entity_stack_retains_animation_payloads: fixture unavailable; requires pinned compiled vanilla entity references from make assets"
        );
        return;
    };
    let Ok(vanilla_bytes) = std::fs::read(local_entity_path()) else {
        eprintln!(
            "skipping captured_entity_stack_retains_animation_payloads: fixture unavailable; requires pinned compiled entity carrier from make assets"
        );
        return;
    };
    let vanilla = assets::RuntimeEntityAssets::decode(&vanilla_bytes).unwrap();
    let view = resource_pack::LayeredPackView::new(stack);
    let files = super::collect::collect_files(&view, Some(&refs), None);
    let compiled = pack_compiler::compile_entity_pack(files.clone())
        .unwrap()
        .expect("captured stack contains entity assets")
        .assets;
    eprintln!(
        "compiled stack: sources={} symbols={} geometries={} clips={}/{} channels={}/{} keyframes={}/{} controllers={} rigs={}",
        compiled.sources.len(),
        compiled.symbols.len(),
        compiled.geometries.len(),
        compiled.animation_clips.len(),
        assets::MAX_ENTITY_ANIMATION_CLIPS,
        compiled.animation_channels.len(),
        assets::MAX_ENTITY_ANIMATION_CHANNELS,
        compiled.animation_keyframes.len(),
        assets::MAX_ENTITY_ANIMATION_KEYFRAMES,
        compiled.controllers.len(),
        compiled.rig_bindings.len(),
    );
    let runtime = assets::RuntimeEntityAssets::from_compiled(compiled)
        .expect("captured entity stack retains its compiled animation payloads");
    let actors = pack_compiler::compile_actor_pack(files.clone())
        .expect("captured entity stack compiles its actor artwork")
        .expect("captured entity stack contains actor artwork");
    let mut fallback_reasons = std::collections::BTreeMap::new();
    for fallback in &actors.fallbacks {
        *fallback_reasons
            .entry(fallback.reason.as_ref())
            .or_insert(0usize) += 1;
    }
    eprintln!(
        "compiled actor stack: bindings={} textures={} skipped={:?} fallbacks={fallback_reasons:?}",
        actors.bindings.len(),
        actors.textures.len(),
        actors.skipped,
    );
    assert!(!actors.bindings.is_empty());
    report_title_rigs(&vanilla, &actors, &files);
    let pages =
        render::ActorArtworkPages::default().with_pack_artwork(&actors.textures, &actors.bindings);
    eprintln!(
        "compiled actor artwork: pages={} rejected_bindings={}",
        pages.pages().len(),
        pages.rejected_bindings(),
    );
    let geometries = render_model::pack_geometries(&runtime);
    let vertices = geometries
        .iter()
        .map(|geometry| geometry.vertices.len())
        .sum::<usize>();
    eprintln!(
        "compiled actor geometry: geometries={} vertices={}/{}",
        geometries.len(),
        vertices,
        render_model::MAX_ACTOR_RIG_VERTICES,
    );
    let equipment = assets::RuntimeEquipmentCatalog::from_parts(
        actors.identity,
        actors.equipment_bindings,
        actors.equipment_textures,
    )
    .unwrap();
    let equipment_geometries =
        client_presentation::presentation::equipment::EquipmentRuntime::pack_geometries(
            &runtime, &equipment,
        );
    let equipment_vertices = equipment_geometries
        .iter()
        .map(|geometry| geometry.vertices.len())
        .sum::<usize>();
    let mut scene = render::ActorRenderScene::with_runtime_entity_assets(&vanilla).unwrap();
    scene.reset();
    let vanilla_vertices = scene.frame().rig.geometry_vertices.len();
    let (entities_result, equipment_result) =
        scene.replace_session_pack_geometries(Some(&runtime), equipment_geometries);
    scene.reset();
    let published_vertices = scene.frame().rig.geometry_vertices.len();
    eprintln!(
        "compiled actor publication: vanilla_vertices={vanilla_vertices} pack_vertices={vertices} equipment_vertices={equipment_vertices} published_vertices={published_vertices} entities={entities_result:?} equipment={equipment_result:?}",
    );
    entities_result.expect("captured entity geometry publishes alongside the vanilla catalog");
    equipment_result.expect("captured equipment geometry publishes alongside the actor catalog");
    assert_eq!(pages.rejected_bindings(), 0);
}

fn report_title_rigs(
    vanilla: &assets::RuntimeEntityAssets,
    actors: &pack_compiler::ActorPackCompilation,
    files: &[(Box<str>, Vec<u8>)],
) {
    use std::sync::Arc;

    use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent};

    let description = files
        .iter()
        .filter(|(path, _)| path.starts_with("entity/"))
        .filter_map(|(_, bytes)| super::super::resource_packs::parse_pack_json(bytes))
        .map(|value| value["minecraft:client_entity"]["description"].clone())
        .find(|description| description["identifier"] == "hivehub:game_hologram")
        .expect("captured stack contains the game hologram definition");
    let plate_controller = description["render_controllers"][0].as_str().unwrap();
    let plate = files
        .iter()
        .filter(|(path, _)| path.starts_with("render_controllers/"))
        .filter_map(|(_, bytes)| super::super::resource_packs::parse_pack_json(bytes))
        .find_map(|value| value["render_controllers"].get(plate_controller).cloned())
        .expect("captured stack contains the hologram plate controller");
    let expected = plate["arrays"]["geometries"]["array.geometries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let alias = entry.as_str().unwrap().to_ascii_lowercase();
            let alias = alias.strip_prefix("geometry.").unwrap();
            description["geometry"][alias].as_str().unwrap().to_owned()
        })
        .collect::<Vec<_>>();
    let entities =
        Arc::new(assets::RuntimeEntityAssets::from_compiled(actors.entities.clone()).unwrap());
    let candidates = actors
        .bindings
        .iter()
        .map(|binding| binding.geometry_candidate)
        .collect();
    let eye = [0.0, 66.0, 8.0];
    let mut world = chunk_pipeline::WorldStream::new_with_asset_sets(
        protocol::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: eye,
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::new(vanilla.clone()),
        eye,
        None,
    );
    world.set_pack_entities(Some((Arc::clone(&entities), candidates)));
    world.set_actor_camera_position(eye);
    for index in 0..=expected.len() {
        let logo = index == expected.len();
        let identifier = if logo {
            "hivehub:logo"
        } else {
            "hivehub:game_hologram"
        };
        let metadata = [
            ActorMetadata {
                key: 2,
                value: ActorMetadataValue::Int(123),
            },
            ActorMetadata {
                key: 43,
                value: ActorMetadataValue::Int(index as i32),
            },
            ActorMetadata {
                key: 38,
                value: ActorMetadataValue::Float(1.0),
            },
        ];
        world
            .submit(
                index as u64 + 1,
                protocol::WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: -(index as i64 + 100),
                    runtime_id: index as u64 + 100,
                    kind: ActorKind::Entity {
                        identifier: identifier.into(),
                    },
                    position: [0.0, 64.0, 0.0],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from(metadata),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    }
    world.advance_actor_interpolation_ticks(20);
    eprintln!(
        "title actor evaluation: variants={} stats={:?}",
        expected.len(),
        world.authority().actor_animation_stats(),
    );
    for index in 0..=expected.len() {
        let actor = world.authority().actor(index as u64 + 100).unwrap();
        let rig = world.authority().actor_rig(actor.runtime_id).unwrap();
        let (catalog, base_geometry) = rig.geometry_source().unwrap();
        let geometry = &catalog.geometries()[base_geometry].identifier;
        let layers = rig
            .render
            .iter()
            .map(|layer| {
                (
                    &catalog.sources()[layer.source as usize].path,
                    layer
                        .geometry
                        .map(|index| &catalog.geometries()[index as usize].identifier),
                    layer.material,
                    layer.color,
                )
            })
            .collect::<Vec<_>>();
        let changed_bones = rig
            .current
            .iter()
            .zip(rig.rest)
            .filter(|(current, rest)| current != rest)
            .count();
        eprintln!(
            "title actor: kind={:?} mark_variant={index} rig={:?} geometry={geometry} fallback={:?} bones={} changed_bones={changed_bones} layers={layers:?}",
            actor.kind,
            rig.rig,
            rig.fallback,
            rig.current.len(),
        );
        let body_rig = render_model::EntityRigId(rig.rig.0);
        assert!(render_model::is_pack_rig_id(body_rig));
        let first = rig
            .render
            .first()
            .expect("title actor retains its render layers");
        let selected = first
            .geometry
            .map_or(base_geometry, |geometry| geometry as usize);
        assert_eq!(
            catalog.geometries()[selected].identifier.as_ref(),
            expected
                .get(index)
                .map_or("geometry.hive.hub.logo", String::as_str),
            "authored title geometry at mark_variant={index}",
        );
        for layer in rig.render {
            let selected = layer
                .geometry
                .map_or(base_geometry, |geometry| geometry as usize);
            let draw_rig = layer.geometry.map_or(body_rig, |geometry| {
                render_model::layer_geometry_rig_id(body_rig, geometry)
            });
            let model = render_model::entity_geometry(catalog, selected, draw_rig)
                .expect("selected title geometry builds in its owning catalog");
            let (previous, current) = if layer.geometry.is_none() && layer.pose.is_empty() {
                (rig.previous, rig.current)
            } else {
                (layer.previous_pose.as_ref(), layer.pose.as_ref())
            };
            assert_eq!(previous.len(), model.bone_pivots.len());
            assert_eq!(current.len(), model.bone_pivots.len());
            assert!(
                layer
                    .hidden_bones
                    .iter()
                    .all(|bone| (*bone as usize) < current.len())
            );
            assert!(previous.iter().chain(current).all(|bone| {
                bone.rotation
                    .iter()
                    .chain(&bone.translation_scale)
                    .chain(&bone.axis_scale)
                    .all(|value| value.is_finite())
            }));
        }
        assert!(rig.current.iter().all(|bone| {
            bone.rotation
                .iter()
                .chain(&bone.translation_scale)
                .chain(&bone.axis_scale)
                .all(|value| value.is_finite())
        }));
    }
}

fn local_vanilla_refs() -> Option<assets::VanillaEntityRefs> {
    let refs = local_entity_path().with_extension("vanillarefs.json");
    assets::VanillaEntityRefs::from_json(&std::fs::read(refs).ok()?)
}

fn local_entity_path() -> std::path::PathBuf {
    let world = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate::asset_startup::DEFAULT_ASSET_PATH);
    crate::asset_startup::entity_asset_path(&world)
}

/// Env-gated: `CINNABAR_PACKCACHE_DIR` names a directory of cached `<uuid>_<version>.zip`
/// packs; prints which pack entities compile to artwork and why the rest fall back.
#[test]
fn report_local_pack_entities() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping report_local_pack_entities: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let mut zips = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .collect::<Vec<_>>();
    zips.sort();
    let refs = local_vanilla_refs();
    eprintln!("vanilla refs loaded: {}", refs.is_some());
    for path in zips {
        let Some(view) = super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let files = super::collect::collect_files(&view, refs.as_ref(), None);
        let entity_files = files
            .iter()
            .filter(|(p, _)| p.starts_with("entity/"))
            .count();
        if entity_files == 0 {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        diagnose_references(&name, &files);
        match pack_compiler::compile_actor_pack(files) {
            Ok(Some(c)) => {
                let mut reasons = std::collections::BTreeMap::<String, Vec<String>>::new();
                for fallback in &c.fallbacks {
                    let rig = &c.entities.rig_bindings[fallback.rig as usize];
                    let id = &c.entities.symbols[rig.entity_symbol as usize].identifier;
                    reasons
                        .entry(fallback.reason.to_string())
                        .or_default()
                        .push(id.to_string());
                }
                let rigs = c.entities.rig_bindings.len();
                eprintln!(
                    "PACK {name}: entity_files={entity_files} rigs={rigs} artwork={} skipped={:?}",
                    c.bindings.len(),
                    c.skipped
                );
                for (reason, ids) in reasons {
                    eprintln!(
                        "  {reason}: {} e.g. {:?}",
                        ids.len(),
                        &ids[..ids.len().min(3)]
                    );
                }
            }
            Ok(None) => eprintln!("PACK {name}: entity_files={entity_files} compiled to nothing"),
            Err(error) => eprintln!("PACK {name}: entity_files={entity_files} ERROR {error}"),
        }
    }
}

/// Counts entity references the pack's own files cannot satisfy (vanilla-defined or missing).
fn diagnose_references(name: &str, files: &[(Box<str>, Vec<u8>)]) {
    use std::collections::{BTreeMap, BTreeSet};
    let json = |bytes: &[u8]| super::super::resource_packs::parse_pack_json(bytes);
    let mut defined = BTreeSet::new();
    let paths = files
        .iter()
        .map(|(p, _)| p.as_ref())
        .collect::<BTreeSet<_>>();
    for (path, bytes) in files {
        if path.starts_with("render_controllers/")
            && let Some(root) = json(bytes)
            && let Some(map) = root["render_controllers"].as_object()
        {
            defined.extend(map.keys().cloned());
        }
    }
    let mut missing_controllers = BTreeMap::<String, u32>::new();
    let mut missing_textures = BTreeMap::<String, u32>::new();
    for (path, bytes) in files {
        if !path.starts_with("entity/") {
            continue;
        }
        let Some(root) = json(bytes) else { continue };
        let description = &root["minecraft:client_entity"]["description"];
        for entry in description["render_controllers"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let key = entry
                .as_str()
                .map(str::to_owned)
                .or_else(|| entry.as_object()?.keys().next().cloned());
            if let Some(key) = key
                && !defined.contains(&key)
            {
                *missing_controllers.entry(key).or_default() += 1;
            }
        }
        for texture in description["textures"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(_, v)| v.as_str())
        {
            let present = [".png", ".tga"]
                .iter()
                .any(|ext| paths.contains(format!("{texture}{ext}").as_str()));
            if !present {
                let dir = texture.rsplit_once('/').map_or("", |(d, _)| d);
                *missing_textures.entry(dir.to_owned()).or_default() += 1;
            }
        }
    }
    eprintln!(
        "  refs {name}: undefined_controllers={missing_controllers:?} textures_not_collected_by_dir={missing_textures:?}"
    );
}
