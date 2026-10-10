//! Env-gated offline render of a cached server pack's entities to PNG frames.

use std::{path::Path, sync::Arc};

use assets::{RuntimeAssets, RuntimeEntityAssets};
use chunk_pipeline::WorldStream;
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap,
    WorldEvent,
};
use render::{ActorArtworkPages, ActorRenderScene};

use client_presentation::presentation::{actors, entity_layers};

const SIDE: u32 = 320;

/// `CINNABAR_RENDER_PACK` names a `<uuid>_<version>.zip`, `CINNABAR_RENDER_OUT` a directory and
/// `CINNABAR_RENDER_ACTORS` `identifier[@variant][*scale]` entries separated by commas; each
/// actor is drawn front-on, textured, into `<n>_<identifier>.png`.
#[test]
fn render_local_pack_entities() {
    let (Some(pack), Some(out), Some(actors)) = (
        std::env::var_os("CINNABAR_RENDER_PACK"),
        std::env::var_os("CINNABAR_RENDER_OUT"),
        std::env::var("CINNABAR_RENDER_ACTORS").ok(),
    ) else {
        eprintln!(
            "skipping render_local_pack_entities: fixture unavailable; offline image export; requires CINNABAR_RENDER_PACK, CINNABAR_RENDER_OUT and CINNABAR_RENDER_ACTORS"
        );
        return;
    };
    assert!(!actors.trim().is_empty(), "fixture must name an actor");
    if !Path::new(&pack).exists() {
        eprintln!(
            "skipping entity render fixture test: CINNABAR_RENDER_PACK names missing {}",
            Path::new(&pack).display()
        );
        return;
    }
    let LocalPack {
        entities,
        artwork,
        candidates,
        textures: compiled_textures,
        bindings: compiled_bindings,
    } = compile_local_pack(Path::new(&pack));
    if let Some(carriers) = std::env::var_os("CINNABAR_RENDER_CARRIERS") {
        measure_pages(Path::new(&carriers), &compiled_textures, &compiled_bindings);
    }
    std::fs::create_dir_all(&out).unwrap();
    for (index, entry) in actors.split(',').enumerate() {
        let (entry, scale) = entry
            .split_once('*')
            .map_or((entry, 1.0), |(id, scale)| (id, scale.parse().unwrap()));
        let (identifier, variant) = entry
            .split_once('@')
            .map_or((entry, 0), |(id, variant)| (id, variant.parse().unwrap()));
        let world = spawn(&entities, &candidates, identifier, variant, scale);
        let image = draw(&world, &entities, &artwork);
        let name = identifier.replace(':', "_");
        image
            .save(Path::new(&out).join(format!("{index:02}_{name}.png")))
            .unwrap();
    }
}

/// A cached pack's entity catalog with its artwork pages.
pub(super) struct LocalPack {
    pub(super) entities: Arc<RuntimeEntityAssets>,
    pub(super) artwork: ActorArtworkPages,
    pub(super) candidates: Vec<u32>,
    textures: Vec<assets::ActorTexture>,
    bindings: Vec<assets::ActorArtworkBinding>,
}

pub(super) fn compile_local_pack(pack: &Path) -> LocalPack {
    let view = super::super::local_pack::local_pack_view_at(pack).unwrap();
    let refs = std::fs::read("../.local/assets/compiled/vanilla-v1.vanillarefs.json")
        .ok()
        .and_then(|bytes| assets::VanillaEntityRefs::from_json(&bytes));
    let compiled = pack_compiler::compile_actor_pack(super::collect::collect_files(
        &view,
        refs.as_ref(),
        None,
    ))
    .unwrap()
    .unwrap();
    let artwork =
        ActorArtworkPages::default().with_pack_artwork(&compiled.textures, &compiled.bindings);
    let candidates = compiled
        .bindings
        .iter()
        .map(|binding| binding.geometry_candidate)
        .collect();
    LocalPack {
        textures: compiled.textures,
        bindings: compiled.bindings,
        entities: Arc::new(RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        artwork,
        candidates,
    }
}

/// A world streaming only `entities`, with the viewer at `eye`.
pub(super) fn world_for(
    entities: &Arc<RuntimeEntityAssets>,
    candidates: &[u32],
    eye: [f32; 3],
) -> WorldStream {
    let mut world = WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: eye,
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        Arc::clone(entities),
        eye,
        None,
    );
    world.set_pack_entities(Some((Arc::clone(entities), candidates.to_vec())));
    world.set_actor_camera_position(eye);
    world
}

fn spawn(
    entities: &Arc<RuntimeEntityAssets>,
    candidates: &[u32],
    identifier: &str,
    variant: i32,
    scale: f32,
) -> WorldStream {
    let mut world = world_for(entities, candidates, [0.0, 66.0, 8.0]);
    let metadata = [
        ActorMetadata {
            key: 2,
            value: ActorMetadataValue::Int(variant),
        },
        ActorMetadata {
            key: 38,
            value: ActorMetadataValue::Float(scale),
        },
    ];
    world
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: -42,
                runtime_id: 42,
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
    world.advance_actor_interpolation_ticks(20);
    world
}

struct Triangle {
    points: [[f32; 3]; 3],
    uvs: [[f32; 2]; 3],
    back_uvs: [[f32; 2]; 3],
    page: usize,
    layer: usize,
    uv_anim: [f32; 4],
}

fn draw(
    world: &WorldStream,
    entities: &RuntimeEntityAssets,
    artwork: &ActorArtworkPages,
) -> image::RgbaImage {
    let mut image = image::RgbaImage::from_pixel(SIDE, SIDE, image::Rgba([40, 44, 52, 255]));
    let (Some(rig), Some(actor)) = (world.authority().actor_rig(42), world.authority().actor(42))
    else {
        eprintln!("render: no rig");
        return image;
    };
    let Some(body) = client_presentation::presentation::actors::entity_rig_presentation(
        &rig, actor, artwork, 0.0,
    ) else {
        eprintln!(
            "render: no presentation for rig {:?} fallback {:?}",
            rig.rig, rig.fallback
        );
        return image;
    };
    let mut batch = client_presentation::presentation::actors::select_actor_presentations(
        1,
        false,
        None,
        [body],
    );
    client_presentation::presentation::entity_layers::apply_render_layers(
        &mut batch,
        |id| world.authority().actor_rig(id),
        artwork,
    );
    let mut scene = ActorRenderScene::default();
    scene.replace_pack_entities(Some(entities)).unwrap();
    scene.configure_artwork(artwork.clone());
    let frame =
        scene.update_rigs_with_artwork(1.0, None, batch.submissions.clone(), &[], &batch.artwork);
    let rig = &frame.rig;
    eprintln!(
        "render: submissions={} instances={} rejects={:?} layers={}",
        batch.submissions.len(),
        rig.instances.len(),
        rig.rejects,
        world
            .authority()
            .actor_rig(42)
            .map_or(0, |rig| rig.render.len()),
    );
    let mut triangles = Vec::new();
    for (instance, entry) in rig.instances.iter().zip(rig.manifest.iter()) {
        let Some(location) = batch.artwork.get(&entry.identity) else {
            continue;
        };
        let span = rig.geometry_spans[instance.geometry_id as usize];
        let Some(vertices) = rig.geometry_vertices.span(span) else {
            continue;
        };
        for corners in vertices.chunks_exact(3) {
            let points = std::array::from_fn(|corner| {
                let vertex = corners[corner];
                let bone =
                    rig.current_bones[(instance.current_bone_base + vertex.bone_index) as usize];
                let posed = apply(&bone, vertex.position);
                apply(&instance.world_from_actor, posed)
            });
            triangles.push(Triangle {
                points,
                uvs: std::array::from_fn(|corner| corners[corner].uv),
                back_uvs: std::array::from_fn(|corner| corners[corner].back_uv),
                page: usize::from(location.page()),
                layer: location.layer() as usize,
                uv_anim: instance.uv_anim,
            });
        }
    }
    rasterize(&mut image, &triangles, artwork);
    image
}

fn apply(rows: &[[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        rows[axis][0] * point[0]
            + rows[axis][1] * point[1]
            + rows[axis][2] * point[2]
            + rows[axis][3]
    })
}

fn rasterize(image: &mut image::RgbaImage, triangles: &[Triangle], artwork: &ActorArtworkPages) {
    let points = triangles.iter().flat_map(|triangle| triangle.points);
    let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
    for point in points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let extent = (high[0] - low[0]).max(high[1] - low[1]).max(1e-3);
    let scale = SIDE as f32 * 0.9 / extent;
    let centre = [(low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5];
    let project = |point: [f32; 3]| {
        [
            SIDE as f32 * 0.5 + (point[0] - centre[0]) * scale,
            SIDE as f32 * 0.5 - (point[1] - centre[1]) * scale,
            point[2],
        ]
    };
    let mut depth = vec![f32::MIN; (SIDE * SIDE) as usize];
    for triangle in triangles {
        let Some(page) = triangle
            .page
            .checked_sub(1)
            .and_then(|page| artwork.pages().get(page))
        else {
            continue;
        };
        let [a, b, c] = triangle.points.map(project);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-6 {
            continue;
        }
        // Counter-clockwise with Y up faces the camera; this screen's Y points down.
        let uvs = if area < 0.0 {
            triangle.uvs
        } else if triangle.back_uvs[0][0] < -1.0e8 {
            continue;
        } else {
            triangle.back_uvs
        };
        let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as u32;
        let max_x = a[0].max(b[0]).max(c[0]).ceil().min(SIDE as f32 - 1.0) as u32;
        let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as u32;
        let max_y = a[1].max(b[1]).max(c[1]).ceil().min(SIDE as f32 - 1.0) as u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let w0 = ((b[0] - p[0]) * (c[1] - p[1]) - (b[1] - p[1]) * (c[0] - p[0])) / area;
                let w1 = ((c[0] - p[0]) * (a[1] - p[1]) - (c[1] - p[1]) * (a[0] - p[0])) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let slot = (y * SIDE + x) as usize;
                if z < depth[slot] {
                    continue;
                }
                let [u, v] = std::array::from_fn(|axis| {
                    let uv = w0 * uvs[0][axis] + w1 * uvs[1][axis] + w2 * uvs[2][axis];
                    let animated = triangle.uv_anim[axis] + uv * triangle.uv_anim[axis + 2];
                    if triangle.uv_anim == [0.0, 0.0, 1.0, 1.0] {
                        animated.clamp(0.0, 1.0)
                    } else {
                        animated.rem_euclid(1.0)
                    }
                });
                let (width, height) = page.dimensions();
                let tx = ((u * f32::from(width)) as usize).min(usize::from(width) - 1);
                let ty = ((v * f32::from(height)) as usize).min(usize::from(height) - 1);
                let at =
                    ((triangle.layer * usize::from(height) + ty) * usize::from(width) + tx) * 4;
                let texel = &page.pixels()[at..at + 4];
                if texel[3] < 26 {
                    continue;
                }
                depth[slot] = z;
                image.put_pixel(x, y, image::Rgba([texel[0], texel[1], texel[2], 255]));
            }
        }
    }
}

/// Prints the actor page count and bytes of the startup artwork (vanilla actor, equipment,
/// icon, world and block-entity carriers in `dir`) and after the pack's artwork joins it.
fn measure_pages(
    dir: &Path,
    textures: &[assets::ActorTexture],
    bindings: &[assets::ActorArtworkBinding],
) {
    let read = |name: &str| std::fs::read(dir.join(name)).unwrap();
    let entity_bytes = read("vanilla-v1.mcbeent");
    let entities = Arc::new(RuntimeEntityAssets::decode(&entity_bytes).unwrap());
    let catalog =
        assets::RuntimeActorCatalog::decode(&read("vanilla-v1.mcbeact"), &entities).unwrap();
    let equipment = assets::RuntimeEquipmentCatalog::decode(&read("vanilla-v1.mcbeeqp")).ok();
    let icons = assets::RuntimeIconCatalog::decode(&read("vanilla-v1.mcbeico")).unwrap();
    let world = std::fs::read(dir.join("vanilla-v2193.mcbea"))
        .ok()
        .and_then(|bytes| RuntimeAssets::decode(&bytes).ok());
    let block_entities = std::fs::read(dir.join("vanilla-v1.mcbeben"))
        .ok()
        .and_then(|bytes| assets::RuntimeBlockEntityAssets::decode(&bytes).ok());
    let summary = |label: &str, pages: &ActorArtworkPages| {
        let bytes: usize = pages.pages().iter().map(|page| page.pixels().len()).sum();
        eprintln!(
            "pages {label}: generic={} bytes={:.1}MiB rejected_bindings={}",
            pages.pages().len(),
            bytes as f64 / 1_048_576.0,
            pages.rejected_bindings()
        );
    };
    let base = ActorArtworkPages::new(&catalog);
    summary("vanilla actor", &base);
    let (_, base, _) = client_presentation::presentation::equipment::EquipmentRuntime::build(
        entities,
        equipment.map(Arc::new),
        Arc::new(icons),
        world.map(Arc::new),
        block_entities.map(Arc::new),
        base,
    );
    summary("startup (actor+equipment+icons+blocks)", &base);
    summary("with pack", &base.with_pack_artwork(textures, bindings));
}
