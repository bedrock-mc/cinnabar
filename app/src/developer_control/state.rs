//! State queries and the conditions `wait_for` polls each frame.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use client_ui::ui_runtime::{
    UiRuntime, presentation::forms::scene_policy::MenuScene, scene_stack::SceneHost,
};
use developer_control::{protocol::Condition, server::Reply};
use serde_json::{Value, json};

use super::{camera::ScriptedCamera, capture::Recording};
use crate::{local_player::LocalViewPose, menu::MenuRuntime, runtime::world::ClientWorld};

/// Waits without a timeout give up after this long so a controller is never wedged.
const DEFAULT_WAIT: Duration = Duration::from_secs(120);
/// Actors listed in a state snapshot, nearest first.
const MAX_LISTED_ACTORS: usize = 32;
/// Largest `chunks_loaded` radius, in columns.
const MAX_CHUNK_RADIUS: u32 = 64;

struct Wait {
    condition: Condition,
    deadline: Instant,
    reply: Reply,
}

#[derive(Resource, Default)]
pub(super) struct Waits(Vec<Wait>);

pub(super) fn configure(app: &mut App) {
    app.init_resource::<Waits>()
        .add_systems(First, poll_waits.after(super::dispatch));
}

pub(super) fn wait(world: &mut World, condition: Condition, timeout_ms: Option<u64>, reply: Reply) {
    if let Condition::ChunksLoaded { radius } = condition
        && radius > MAX_CHUNK_RADIUS
    {
        reply.send(Err(format!("radius must be at most {MAX_CHUNK_RADIUS}")));
        return;
    }
    let timeout = timeout_ms.map_or(DEFAULT_WAIT, Duration::from_millis);
    world.resource_mut::<Waits>().0.push(Wait {
        condition,
        deadline: Instant::now() + timeout,
        reply,
    });
}

fn poll_waits(world: &mut World) {
    let waits = std::mem::take(&mut world.resource_mut::<Waits>().0);
    let now = Instant::now();
    let mut kept = Vec::with_capacity(waits.len());
    for wait in waits {
        match evaluate(world, &wait.condition) {
            Ok(detail) => wait
                .reply
                .send(Ok(json!({ "met": true, "detail": detail }))),
            Err(detail) if now >= wait.deadline => wait.reply.send(Err(format!(
                "timed out waiting for {:?}: {detail}",
                wait.condition
            ))),
            Err(_) => kept.push(wait),
        }
    }
    world.resource_mut::<Waits>().0.extend(kept);
}

/// `Ok` with what satisfied the condition, else `Err` with how far off it is.
fn evaluate(world: &World, condition: &Condition) -> Result<Value, String> {
    match condition {
        Condition::InWorld => player_position(world)
            .map(|position| json!({ "position": position }))
            .ok_or_else(|| "no world yet".to_owned()),
        Condition::ChunksLoaded { radius } => {
            let stream = world
                .resource::<ClientWorld>()
                .stream
                .as_ref()
                .ok_or("no world yet")?;
            let radius = u16::try_from(*radius).unwrap_or(u16::MAX);
            let position = world.get_resource::<LocalViewPose>().map_or_else(
                || stream.authority().resolved_server_position().position,
                |view| view.feet_translation().to_array(),
            );
            let (loaded, total) = stream.loaded_columns_around(position, radius);
            let detail = json!({ "loaded": loaded, "total": total });
            if loaded == total {
                Ok(detail)
            } else {
                Err(detail.to_string())
            }
        }
        Condition::Screen { name } => {
            let screens = screens(world);
            let top = screens.last().cloned().unwrap_or_default();
            if top.contains(name.as_str()) {
                Ok(json!({ "top": top }))
            } else {
                Err(format!("top screen is {top:?}"))
            }
        }
        Condition::Entity { matches, radius } => {
            let found = actors(world)
                .into_iter()
                .find(|actor| {
                    let named = ["type", "name"].iter().any(|field| {
                        actor[*field]
                            .as_str()
                            .is_some_and(|value| value.contains(matches.as_str()))
                    });
                    let near = radius.is_none_or(|radius| {
                        actor["distance"]
                            .as_f64()
                            .is_some_and(|distance| distance <= f64::from(radius))
                    });
                    named && near
                })
                .ok_or_else(|| format!("no actor matching {matches:?}"))?;
            Ok(found)
        }
        Condition::CameraFinished => match world.get_resource::<ScriptedCamera>() {
            Some(camera) if !camera.finished() => Err(camera.summary().to_string()),
            _ => Ok(json!({})),
        },
    }
}

fn player_position(world: &World) -> Option<[f32; 3]> {
    let stream = world.resource::<ClientWorld>().stream.as_ref()?;
    Some(stream.authority().resolved_server_position().position)
}

/// Screen names bottom first, e.g. `Gameplay`, `Hud`, `Chat`, `Menu(Pause)`.
fn screens(world: &World) -> Vec<String> {
    let (Some(ui), Some(player)) = (
        world.get_resource::<UiRuntime>(),
        world.get_resource::<crate::player_runtime::PlayerRuntime>(),
    ) else {
        return Vec::new();
    };
    let menu = world
        .get_resource::<MenuRuntime>()
        .map(|menu| menu as &dyn MenuScene);
    ui.scenes(player, SceneHost::of(menu, ui))
        .scenes()
        .iter()
        .map(|entry| format!("{:?}", entry.key))
        .collect()
}

fn actors(world: &World) -> Vec<Value> {
    let Some(stream) = world.resource::<ClientWorld>().stream.as_ref() else {
        return Vec::new();
    };
    let eye = world
        .get_resource::<LocalViewPose>()
        .map_or(Vec3::ZERO, |view| view.eye_translation());
    let authority = stream.authority();
    let mut actors: Vec<(f32, Value)> = authority
        .remote_actors()
        .map(|actor| {
            let distance = Vec3::from_array(actor.position).distance(eye);
            let (kind, name) = match &actor.kind {
                protocol::ActorKind::Player { username, .. } => {
                    ("minecraft:player".to_owned(), Some(username.to_string()))
                }
                protocol::ActorKind::Entity { identifier } => (
                    identifier.to_string(),
                    authority
                        .actor_name_tag(actor.unique_id)
                        .map(|name| name.to_string()),
                ),
            };
            let health = authority.actor_health_by_unique(actor.unique_id);
            (
                distance,
                json!({
                    "unique_id": actor.unique_id,
                    "runtime_id": actor.runtime_id,
                    "type": kind,
                    "name": name,
                    "position": actor.position,
                    "distance": distance,
                    "health": health,
                }),
            )
        })
        .collect();
    actors.sort_by(|a, b| a.0.total_cmp(&b.0));
    actors.into_iter().map(|(_, actor)| actor).collect()
}

pub(super) fn snapshot(world: &World) -> Value {
    let view = world.get_resource::<LocalViewPose>();
    let stream = world.resource::<ClientWorld>().stream.as_ref();
    let health = world
        .get_resource::<UiRuntime>()
        .and_then(|ui| ui.hud().health())
        .map(|stat| {
            let scale = f32::from(stat.scale());
            json!({
                "current": f32::from(stat.current()) / scale,
                "maximum": f32::from(stat.maximum()) / scale,
            })
        });
    let mut actors = actors(world);
    let actor_count = actors.len();
    actors.truncate(MAX_LISTED_ACTORS);
    let menu = world.get_resource::<MenuRuntime>();
    json!({
        "in_world": stream.is_some(),
        "immobile": world.get_resource::<crate::player_runtime::PlayerRuntime>()
            .map(|player| player.facts.is_immobile()),
        "feet": view.map(|view| view.feet_translation().to_array()),
        "eye": view.map(|view| view.eye_translation().to_array()),
        "server_position": player_position(world),
        "rotation": view.map(|view| {
            let [yaw, pitch] = super::input::view_angles(view);
            json!({ "yaw": yaw, "pitch": pitch })
        }),
        "health": health,
        "dimension": stream.map(|stream| stream.current_dimension()),
        "chunks": stream.map(|stream| json!({
            "loaded_columns": stream.loaded_column_count(),
            "render_distance_blocks": stream.render_distance_blocks(),
        })),
        "actor_count": actor_count,
        "actors": actors,
        "actor_draw": super::actors::snapshot(world),
        "primitive_shapes": crate::primitive_shapes::snapshot(world),
        "sidebar": world.get_resource::<UiRuntime>()
            .and_then(|ui| super::scoreboards::snapshot(ui.scoreboards())),
        "screens": screens(world),
        "menu": menu.map(|menu| json!({
            "visible": menu.is_visible(),
            "screen": format!("{:?}", menu.screen()),
            "connecting": menu.is_connecting(),
        })),
        "driven": world.contains_resource::<crate::camera::DrivenInput>(),
        "camera": world.get_resource::<ScriptedCamera>().map(ScriptedCamera::summary),
        "recording": world.get_resource::<Recording>().map(Recording::summary),
        "game_seconds": world.resource::<Time>().elapsed_secs_f64(),
    })
}
