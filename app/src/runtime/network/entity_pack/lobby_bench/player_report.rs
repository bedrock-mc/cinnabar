//! Offline player-body evidence from the real packet, actor and render publication paths.

use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

use bevy::{math::Mat4, prelude::Entity};
use protocol::{ActorEvent, PlayerListEntry, PlayerSkin};
use serde_json::{Value, json};

use super::super::scene_report::{Frame, HEIGHT, WIDTH};
use super::*;
use crate::runtime::world::ClientWorld;

/// Gives players stable anonymous labels in first PlayerList appearance order.
fn labels(capture: &Capture) -> BTreeMap<[u8; 16], String> {
    let mut labels = BTreeMap::new();
    for (_, body) in capture.packets.iter().filter(|(id, _)| *id == 63) {
        let mut packet = vec![63];
        packet.extend_from_slice(body);
        let mut batch = vec![0xfe];
        write_varint(&mut batch, packet.len() as u64);
        batch.extend(packet);
        let packets =
            protocol::decode_batch(batch.into(), &BedrockSession { shield_item_id: 0 }).unwrap();
        for packet in packets {
            if let Ok(Some(WorldEvent::Actor(ActorEvent::PlayerList(list)))) =
                protocol::into_world_event(packet, 0)
            {
                for entry in list.entries.iter() {
                    if let PlayerListEntry::Add { uuid, .. } = entry {
                        let next = labels.len() + 1;
                        labels.entry(*uuid).or_insert_with(|| format!("P{next:02}"));
                    }
                }
            }
        }
    }
    labels
}

/// Compares retained skin payloads without recording usernames or UUIDs in the report.
fn skin_stamp(skin: Option<&PlayerSkin>) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    match skin {
        Some(PlayerSkin::Standard(skin)) => {
            (skin.width, skin.height).hash(&mut hash);
            skin.rgba8.hash(&mut hash);
            if let Some(geometry) = &skin.geometry {
                geometry.resource_patch.hash(&mut hash);
                geometry.geometry_data.hash(&mut hash);
                for layer in geometry.animations.iter() {
                    (
                        layer.kind.slot(),
                        layer.width,
                        layer.height,
                        layer.frames,
                        layer.blinking,
                    )
                        .hash(&mut hash);
                    layer.rgba8.hash(&mut hash);
                }
            }
        }
        other => format!("{other:?}").hash(&mut hash),
    }
    hash.finish()
}

/// Applies the same affine rows as the actor vertex shader.
fn apply(rows: &[[f32; 4]; 3], point: [f32; 3]) -> Vec3 {
    Vec3::from_array(std::array::from_fn(|axis| {
        rows[axis][0] * point[0]
            + rows[axis][1] * point[1]
            + rows[axis][2] * point[2]
            + rows[axis][3]
    }))
}

/// Renders the player's base and animated skin layers from published meshes, poses and artwork.
fn draw_body(rendered: &ActorRenderFrame, runtime_id: u64, out: &Path) -> (usize, usize) {
    let clip = Mat4::perspective_rh(
        45f32.to_radians(),
        WIDTH as f32 / HEIGHT as f32,
        0.05,
        100.0,
    ) * Mat4::look_at_rh(
        Vec3::new(2.7, 1.7, -4.5),
        Vec3::new(0.0, 0.95, 0.0),
        Vec3::Y,
    );
    let mut image = Frame::new(clip);
    let mut triangles = 0;
    for ((instance, entry), page) in rendered
        .rig
        .instances
        .iter()
        .zip(rendered.rig.manifest.iter())
        .zip(rendered.instance_pages())
    {
        if entry.identity.runtime_id != runtime_id
            || (entry.identity.layer != render::ACTOR_LAYER_BODY
                && !crate::presentation::skin_layers::is_skin_layer(entry.identity.layer))
        {
            continue;
        }
        let layer = instance.texture_layer as usize;
        let (width, height, pixels) = if *page == 0 {
            (
                render::STANDARD_SKIN_SIDE,
                render::STANDARD_SKIN_SIDE,
                rendered.skins_rgba8.as_ref(),
            )
        } else {
            let Some(page) = rendered.artwork_pages().pages().get(usize::from(*page) - 1) else {
                continue;
            };
            let (width, height) = page.dimensions();
            (usize::from(width), usize::from(height), page.pixels())
        };
        let bytes = width * height * 4;
        let Some(skin) = pixels.get(layer * bytes..(layer + 1) * bytes) else {
            continue;
        };
        let uv_anim = instance.uv_anim;
        let shade = |uv: [f32; 2]| {
            let [x, y]: [usize; 2] = std::array::from_fn(|axis| {
                let value = uv_anim[axis] + uv[axis] * uv_anim[axis + 2];
                let value = if uv_anim == render::IDENTITY_UV_ANIM {
                    value.clamp(0.0, 1.0)
                } else {
                    value.rem_euclid(1.0)
                };
                let size = [width, height][axis];
                ((value * size as f32) as usize).min(size - 1)
            });
            let at = (y * width + x) * 4;
            (skin[at + 3] >= 26).then(|| [skin[at], skin[at + 1], skin[at + 2], 255])
        };
        let span = rendered.rig.geometry_spans[instance.geometry_id as usize];
        let Some(vertices) = rendered.rig.geometry_vertices.span(span) else {
            continue;
        };
        // Centre each body, keeping its actual published orientation and bone poses.
        let origin = Vec3::from_array(instance.world_from_actor.map(|row| row[3]));
        for corners in vertices.chunks_exact(3) {
            let placed: [(Vec3, [f32; 2]); 3] = std::array::from_fn(|corner| {
                let vertex = corners[corner];
                let index = (instance.current_bone_base + vertex.bone_index) as usize;
                let previous = rendered.rig.previous_bones
                    [(instance.previous_bone_base + vertex.bone_index) as usize];
                let posed = apply(&previous, vertex.position).lerp(
                    apply(&rendered.rig.current_bones[index], vertex.position),
                    instance.partial_tick,
                );
                (
                    apply(&instance.world_from_actor, posed.to_array()) - origin,
                    vertex.uv,
                )
            });
            let projected = placed.map(|(point, _)| {
                let p = clip * point.extend(1.0);
                p.truncate() / p.w
            });
            let [a, b, c] = projected;
            let back = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) <= 0.0;
            if back && corners[0].back_uv[0] < -1.0e8 {
                continue;
            }
            image.triangle(
                std::array::from_fn(|i| {
                    (
                        placed[i].0,
                        if back {
                            corners[i].back_uv
                        } else {
                            placed[i].1
                        },
                    )
                }),
                true,
                true,
                &shade,
            );
            triangles += 1;
        }
    }
    let visible = image
        .image
        .pixels()
        .filter(|p| p.0 != [138, 178, 232, 255])
        .count();
    image.image.save(out).unwrap();
    (triangles, visible)
}

/// Records every newly observed skin/visibility state after the actor publication tick.
fn report_states(
    world: &World,
    labels: &BTreeMap<[u8; 16], String>,
    seen: &mut BTreeMap<String, Vec<(u64, bool)>>,
    records: &mut Vec<Value>,
    packet_index: usize,
    out: &Path,
) {
    let stream = world.resource::<ClientWorld>().stream.as_ref().unwrap();
    let rendered = world.resource::<ActorRenderFrame>();
    for (actor, profile) in stream.authority().render_players() {
        let ActorKind::Player { uuid, .. } = &actor.kind else {
            continue;
        };
        let Some(label) = labels.get(uuid) else {
            continue;
        };
        let skin = profile.map(|profile| &profile.skin);
        let state = (skin_stamp(skin), actor.is_invisible());
        let states = seen.entry(label.clone()).or_default();
        if states.contains(&state) {
            continue;
        }
        states.push(state);
        let file = format!("{label}-state-{:02}.png", states.len());
        let (triangles, pixels) = draw_body(rendered, actor.runtime_id, &out.join(&file));
        let skin = match skin {
            Some(PlayerSkin::Standard(skin)) => json!({
                "width": skin.width, "height": skin.height, "bytes": skin.rgba8.len(),
                "opaque_pixels": skin.rgba8.chunks_exact(4).filter(|p| p[3] == 255).count(),
                "transparent_pixels": skin.rgba8.chunks_exact(4).filter(|p| p[3] == 0).count(),
                "geometry_bytes": skin.geometry.as_ref().map(|g| g.geometry_data.len()),
                "resource_patch": skin.geometry.as_ref().map(|g| g.resource_patch.as_ref()),
            }),
            other => json!({ "unavailable": format!("{other:?}") }),
        };
        let rig = stream.authority().actor_rig(actor.runtime_id);
        records.push(json!({
            "player": label, "state": states.len(), "packet_index": packet_index,
            "skin": skin, "invisible_flag": actor.is_invisible(),
            "rig_present": rig.is_some(), "bones": rig.as_ref().map(|r| r.current.len()),
            "completed_tick": rig.as_ref().map(|r| r.completed_tick),
            "custom_geometry": rig.as_ref().is_some_and(|r| r.skin_geometry.is_some()),
            "animated_layers": rig.as_ref().map(|r| r.skin_layers.len()),
            "body_triangles": triangles, "visible_pixels": pixels, "png": file,
        }));
    }
}

/// Produces before/after evidence without a network connection or terrain/camera culling.
#[test]
#[ignore = "offline evidence; needs a lobby capture, compiled carriers and its cached pack"]
fn lobby_player_report() {
    let capture = read_capture(Path::new(
        &std::env::var_os("CINNABAR_LOBBY_CAPTURE").unwrap(),
    ));
    let pack = std::env::var_os("CINNABAR_RENDER_PACK").unwrap();
    let out = PathBuf::from(std::env::var_os("CINNABAR_PLAYER_REPORT_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let labels = labels(&capture);
    let empty = Capture {
        bootstrap: capture.bootstrap,
        packets: vec![],
    };
    let (mut world, _, mut replay) = build_world(&empty, Path::new(&pack), false);
    let cameras: Vec<Entity> = world
        .query_filtered::<Entity, bevy::prelude::With<crate::camera::FlyCamera>>()
        .iter(&world)
        .collect();
    for camera in cameras {
        world.despawn(camera);
    }
    let mut clock = Instant::now();
    // Prime Time<Real>'s initial zero delta before measuring each packet's full actor tick.
    world
        .resource_mut::<Time<Real>>()
        .update_with_instant(clock);
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    world.run_system_cached(publish_actor_render_frame).unwrap();
    let mut seen = BTreeMap::new();
    let mut records = Vec::new();
    for (index, (id, body)) in capture.packets.iter().enumerate() {
        {
            let mut client = world.resource_mut::<ClientWorld>();
            replay.apply(client.stream.as_mut().unwrap(), *id, body);
            drain_through(
                client.stream.as_mut().unwrap(),
                replay.local_position,
                replay.sequence,
            );
        }
        if ![12, 39, 63, 93].contains(id) {
            continue;
        }
        // Three frames cross one actor tick, ensuring newly arrived geometry has a pose.
        for _ in 0..3 {
            clock += FRAME;
            world
                .resource_mut::<Time<Real>>()
                .update_with_instant(clock);
            world.run_system_cached(prepare_actor_render_frame).unwrap();
            world.run_system_cached(publish_actor_render_frame).unwrap();
        }
        report_states(&world, &labels, &mut seen, &mut records, index, &out);
    }
    std::fs::write(out.join("players.json"), serde_json::to_vec_pretty(&json!({
        "listed_players": labels.len(), "spawned_players": seen.len(), "states": records,
        "rejected_packets": replay.rejected, "rejections": replay.rejections,
        "method": "Real packet decoder, WorldStream, actor publication and skin/mesh/bone buffers; isolated body software raster with alpha testing. No terrain, lighting, name tags, equipment or GPU acceptance claim."
    })).unwrap()).unwrap();
    eprintln!(
        "PLAYER_REPORT players={} states={} rejected={} out={}",
        seen.len(),
        records.len(),
        replay.rejected,
        out.display()
    );
}
