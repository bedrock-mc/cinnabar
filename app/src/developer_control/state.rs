//! State queries and the conditions `wait_for` polls each frame.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::{presentation::forms::scene_policy::MenuScene, scene_stack::SceneHost};
use developer_control::{protocol::Condition, server::Reply};
use serde_json::{Value, json};

use super::{camera::ScriptedCamera, capture::Recording};
use {
    crate::runtime::world::ClientWorld, client_presentation::local_player::LocalViewPose,
    client_shell::menu::MenuRuntime,
};

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
        world.get_resource::<UiRuntimeResource>(),
        world.get_resource::<client_state::PlayerRuntime>(),
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
                protocol::ActorKind::Player { username, .. } => (
                    assets::facts::entity::NativeEntityClass::Player
                        .identifier()
                        .expect("pinned player identity")
                        .to_owned(),
                    Some(username.to_string()),
                ),
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
                    "painting": actor.painting.as_ref().map(|painting| json!({
                        "motive":painting.motive.as_ref(),
                        "direction":format!("{:?}",painting.direction),
                        "size_pixels":assets::painting::motive(&painting.motive).size,
                    })),
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

fn player_motion(world: &World) -> Option<Value> {
    let stream = world.get_resource::<ClientWorld>()?.stream.as_ref()?;
    let player = world.get_resource::<client_state::PlayerRuntime>()?;
    let physics = world.get_resource::<client_interaction::movement::LocalPhysicsController>()?;
    let authority = stream.authority();
    let rig = authority.actor_rig(stream.local_player_runtime_id())?;
    let java = rig.java;
    let state = physics.state();
    let jump_held = world
        .get_resource::<client_state::SemanticInputSnapshot>()
        .and_then(|input| input.snapshot())
        .map(|input| input.phases[semantic_input::Action::Jump as usize].held);
    let item_use = world.get_resource::<client_interaction::item_use::ItemUseRuntime>();
    let selected_item = player.selected_stack().map(|stack| {
        let profile = authority.item_use_profile(stack.network_id);
        json!({
            "network_id": stack.network_id,
            "identifier": authority.canonical_item_stack(stack)
                .and_then(|item| item.identifier)
                .map(|identifier| identifier.to_string()),
            "use_profile": {
                "kind": profile.kind.map(|kind| format!("{kind:?}")),
                "ticks": profile.ticks,
                "consumption": format!("{:?}", profile.consumption),
                "movement_modifier": profile.movement,
            },
        })
    });
    Some(json!({
        "mode": format!("{:?}", physics.mode()),
        "sprinting": physics.latest_sneak_sprint().map(|(_, sprinting)| sprinting),
        "always_sprint": world.get_resource::<client_state::RuntimeSettings>()
            .map(|settings| settings.user_settings_update().1.gameplay.always_sprint),
        "item_use": {
            "active": item_use.map(|item_use| item_use.is_using()),
            "movement_modifier": item_use.and_then(|item_use| item_use.movement_modifier()),
        },
        "selected_item": selected_item,
        "on_ground": state.map(|state| state.on_ground),
        "spin_attack_ticks": state.and_then(|state| state.spin_attack_ticks),
        "tick": state.map(|state| state.tick),
        "velocity": state.map(|state| [state.velocity.x, state.velocity.y, state.velocity.z]),
        "jump_delay": state.map(|state| state.jump_delay),
        "knockback_sequence": physics.knockback_sequence(),
        "physics_authorized": world.get_resource::<client_interaction::movement::MovementTicker>()
            .map(|movement| movement.physics_is_authorized()),
        "jump_eligible": physics.jump_pulse_eligible(),
        "jump_held": jump_held,
        "flying": player.facts.game_mode_capabilities().map(|abilities| abilities.flight.is_flying()),
        "dimension_transfer": world.get_resource::<ClientWorld>()
            .map(|world| world.dimension_transfer.active()),
        "respawn_input_held": world.get_resource::<ClientWorld>()
            .map(|world| world.respawn.input_held()),
        "mount_unique_id": player.facts.mount_unique_id(),
        "java": {
            "walked": java.walked,
            "bob": java.bob,
            "chase": java.cape,
            "body_yaw": java.body_yaw,
            "riding": java.riding,
        },
    }))
}

pub(super) fn snapshot(world: &World) -> Value {
    let view = world.get_resource::<LocalViewPose>();
    let stream = world.resource::<ClientWorld>().stream.as_ref();
    let health = world
        .get_resource::<UiRuntimeResource>()
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
    let menu_snapshot = menu.map(|menu| {
        let view = menu.view();
        json!({
            "visible": menu.is_visible(),
            "screen": format!("{:?}", menu.screen()),
            "connecting": menu.is_connecting(),
            "focused_action": view.focused_action.map(|action| format!("{action:?}")),
            "sign_in_browser": format!("{:?}", view.sign_in_browser),
            "sign_in_fixture": menu.sign_in_fixture_state(),
        })
    });
    let local_mods = crate::modding::developer_state(world);

    json!({
        "in_world": stream.is_some(),
        "sign_in": menu.map(|menu| menu.sign_in_status().as_str()),
        "immobile": world.get_resource::<client_state::PlayerRuntime>()
            .map(|player| player.facts.is_immobile()),
        "feet": view.map(|view| view.feet_translation().to_array()),
        "eye": view.map(|view| view.eye_translation().to_array()),
        "server_position": player_position(world),
        "rotation": view.map(|view| {
            let [yaw, pitch] = super::input::view_angles(view);
            json!({ "yaw": yaw, "pitch": pitch })
        }),
        "health": health,
        "java_animations": world.get_resource::<client_presentation::camera::CameraSettingsAuthority>()
            .map(|settings| settings.feel().java_animations),
        "perspective": world.get_resource::<client_presentation::camera::CameraSettingsAuthority>()
            .map(|settings| format!("{:?}", settings.perspective())),
        "hurt_camera_style": world.get_resource::<client_presentation::camera::CameraSettingsAuthority>()
            .map(|settings| {
                let style = settings.hurt_camera_style();
                json!({"intensity": style.intensity(), "old_tilt": style.old_tilt()})
            }),
        "dimension": stream.map(|stream| stream.current_dimension()),
        "chunks": stream.map(|stream| json!({
            "loaded_columns": stream.loaded_column_count(),
            "render_distance_blocks": stream.render_distance_blocks(),
        })),
        "actor_count": actor_count,
        "actors": actors,
        "actor_draw": super::actors::snapshot(world),
        "player_motion": player_motion(world),
        "local_mods": local_mods,
        "primitive_shapes": crate::primitive_shapes::snapshot(world),
        "sidebar": world.get_resource::<UiRuntimeResource>()
            .and_then(|ui| super::scoreboards::snapshot(ui.scoreboards())),
        "screens": screens(world),
        "menu": menu_snapshot,
        "dressing_room": menu.map(|menu| {
            let view = menu.view();
            let room = &view.dressing_room;
            json!({
                "selected": room.selected,
                "selected_cape": room.selected_cape,
                "section": format!("{:?}", room.section),
                "editor": room.editor.as_ref().map(|editor| json!({"mode": format!("{:?}", editor.mode), "target": format!("{:?}", editor.target), "draft": editor.draft})),
                "capes": room.capes.iter().map(|cape| json!({"id": cape.id, "name": cape.name, "imported": cape.imported})).collect::<Vec<_>>(),
                "busy": room.busy,
                "message": room.message,
                "skins": room.skins.iter().map(|skin| json!({
                    "id": skin.id, "name": skin.name, "model": skin.model,
                    "imported": skin.imported,
                })).collect::<Vec<_>>(),
            })
        }),
        "skin": world.get_resource::<client_shell::player_skin::LocalPlayerSkin>().map(|skin| json!({
            "width": skin.width, "height": skin.height, "arm_size": skin.skin_model.arm_size(),
            "content_hash": skin.rgba8.content_hash(),
            "cape": skin.cape.as_ref().map(|cape| json!({"width": cape.width, "height": cape.height})),
        })),
        "menu_preview": world.get_resource::<client_shell::ui_resources::UiPresentationResource>().map(|ui| json!({
            "angles": ui.menu_player_preview_angles(),
            "bounds": ui.menu_player_preview_bounds().map(|bounds| [
                bounds.min().x(), bounds.min().y(), bounds.max().x(), bounds.max().y(),
            ]),
        })),
        "driven": world.contains_resource::<client_state::DrivenInput>(),
        "camera": world.get_resource::<ScriptedCamera>().map(ScriptedCamera::summary),
        "server_camera": world.get_resource::<client_presentation::camera::ServerCameraView>().map(|camera| json!({
            "active": camera.is_active(),
            "preset": camera.active_preset_name(),
            "base_preset": camera.active_base_preset_name(),
            "pose_override": camera.has_pose_override(),
            "fade": camera.fade_overlay(),
            "skips": format!("{:?}", camera.skips()),
        })),
        "aim_assist": aim_assist(world),
        "cave_visibility": world.get_resource::<crate::runtime::visibility::CaveVisibilityCache>()
            .map(crate::runtime::visibility::CaveVisibilityCache::telemetry_snapshot),
        "recording": world.get_resource::<Recording>().map(Recording::summary),
        "game_seconds": world.resource::<Time>().elapsed_secs_f64(),
        "frame_count": world.get_resource::<bevy::diagnostic::FrameCount>().map(|count| count.0),
        "placement_frames": world.get_resource::<client_interaction::block_use::BlockUseRuntime>().map(|runtime| json!({
            "committed": runtime.prediction_frames.committed_frame,
            "staged": runtime.prediction_frames.staged_frame,
            "upload_acknowledged": runtime.prediction_frames.upload_acknowledged_frame,
        })),
    })
}

/// Publishes camera diagnostics only when the developer explicitly requests a state snapshot.
fn aim_assist(world: &World) -> Option<Value> {
    use client_presentation::aim_assist::{AimAssistFrame, ServerAimAssist};
    let state = world.get_resource::<ServerAimAssist>()?;
    let frame = world.get_resource::<AimAssistFrame>()?;
    Some(json!({
        "enabled": state.settings().is_some(),
        "semantic_skips": state.semantic_skips(),
        "skipped_queries": frame.skipped_queries,
        "visibility_queries": frame.visibility_queries,
        "target": frame.target.map(|target| json!({
            "kind": format!("{:?}", target.kind),
            "point": target.point.to_array(),
        })),
    }))
}
