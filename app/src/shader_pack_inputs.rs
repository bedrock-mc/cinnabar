//! Publishes the game state behind the standard Iris uniforms for the experimental shader-pack
//! render mode. Every value is read from the authority the vanilla renderer and HUD already use;
//! fields without a Cinnabar source keep [`ShaderPackFrameInputs::default`].

use bevy::{
    prelude::{
        App, DetectChangesMut, IntoScheduleConfigs, Plugin, Query, Res, ResMut, Resource, Time,
        Transform, Update, Vec3, With,
    },
    time::Real,
};
use chunk_pipeline::WorldStream;
use client_interaction::movement::LocalPhysicsController;
use client_presentation::{
    camera::{FlyCamera, ServerCameraView, VisionEffects},
    environment::{WorldClock, visual_world_time},
    local_player::LocalPlayerFrameSet,
};
use client_shell::ui_resources::UiRuntimeResource;
use meshing::CameraMedium;
use protocol::NetworkItemStack;
use render_model::celestial;
use render_scene::{
    ActorRenderFrame, AtmosphereFrame, WorldLighting,
    shaderpack::{SKY_BIOME_SLOTS, ShaderPackActorIds, ShaderPackFrameInputs, ShaderPackIdMaps},
};

use crate::{
    environment::{
        CameraMediumState, FixedTime, VisualTimeOverride, rendered_clock, update_atmosphere_frame,
    },
    runtime::world::ClientWorld,
};

/// Highest block or sky light level; light is stored in one nibble.
const MAX_LIGHT_LEVEL: u8 = 15;
/// Iris scales each light level by 16, so `eyeBrightness` spans `0..=240`.
const EYE_BRIGHTNESS_PER_LEVEL: i32 = 16;
/// Length of the lunar cycle in days; the phase advances once per day.
const MOON_PHASES: i64 = 8;
/// Java blends biome sky and fog colours with these Gaussian weights, one per quart (four
/// blocks) along each axis; a sample's weight interpolates between neighbouring entries by
/// the camera's place within its quart.
const BIOME_BLEND_KERNEL: [f32; 7] = [0.0, 1.0, 4.0, 6.0, 4.0, 1.0, 0.0];
/// Quarts sampled per axis around the camera.
const BIOME_BLEND_SPAN: usize = 6;
/// Blocks per quart, the resolution Java stores biomes at.
const QUART_BLOCKS: i32 = 4;

/// Developer-only `--frame-time-counter` pin: `frameTimeCounter` frozen at this many
/// seconds so animations driven by it (BSL's clouds) match a reference capture.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameTimeCounterPin(pub(crate) Option<f32>);

/// Keeps [`ShaderPackFrameInputs`] in step with the game each frame. Builds without the
/// `shaderpacks` feature register nothing, so they do no work.
pub(crate) struct ShaderPackInputsPlugin;

impl Plugin for ShaderPackInputsPlugin {
    fn build(&self, app: &mut App) {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        app.init_resource::<ShaderPackFrameInputs>()
            .init_resource::<FrameTimeCounterPin>()
            .init_resource::<ShaderPackIdMaps>()
            .init_resource::<ShaderPackActorIds>()
            .add_systems(
                Update,
                (
                    publish_frame_inputs
                        .after(update_atmosphere_frame)
                        .after(LocalPlayerFrameSet::Interaction),
                    publish_actor_ids,
                ),
            );
    }
}

/// The rendered clock, atmosphere frame and lightmap inputs the vanilla sky draws from.
type EnvironmentSources<'w> = (
    Res<'w, WorldClock>,
    Option<Res<'w, FixedTime>>,
    Option<Res<'w, VisualTimeOverride>>,
    Res<'w, Time<Real>>,
    Res<'w, AtmosphereFrame>,
    Res<'w, WorldLighting>,
    Res<'w, CameraMediumState>,
);

/// World stream, local-player authority, HUD state, camera-selected status effects and the
/// capture pin.
type PlayerSources<'w> = (
    Res<'w, ClientWorld>,
    Res<'w, client_state::PlayerRuntime>,
    Res<'w, UiRuntimeResource>,
    Res<'w, VisionEffects>,
    Option<Res<'w, ServerCameraView>>,
    Option<Res<'w, LocalPhysicsController>>,
    Res<'w, FrameTimeCounterPin>,
);

/// Rebuilds the inputs from defaults and replaces the resource only when a value changed, so
/// an idle frame raises no change tick.
fn publish_frame_inputs(
    environment: EnvironmentSources,
    player: PlayerSources,
    cameras: Query<&Transform, With<FlyCamera>>,
    ids: Res<ShaderPackIdMaps>,
    mut inputs: ResMut<ShaderPackFrameInputs>,
) {
    let (clock, fixed_time, visual_time, time, frame, lighting, medium) = environment;
    let (client_world, player_runtime, ui, vision, server_camera, physics, pin) = player;
    let mut next = ShaderPackFrameInputs {
        frame_time_counter: pin.0,
        ..ShaderPackFrameInputs::default()
    };

    // Same clock as the vanilla atmosphere draws with.
    let clock = rendered_clock(*clock, fixed_time.as_deref(), visual_time.as_deref());
    let day = DayClock::at(visual_world_time(clock, time.elapsed_secs_f64()));
    next.world_time = day.world_time;
    next.world_day = day.world_day;
    next.moon_phase = day.moon_phase;
    next.celestial_angle = day.celestial_angle;

    let frame = *frame;
    next.rain_strength = frame.rain_level();
    next.thunder_strength = frame.thunder_level();
    next.fog_range = [frame.fog_start(), frame.fog_end()];
    next.dimension = frame.sky_kind() as i32;
    next.eye_medium = eye_medium(medium.0);

    // Lightmap inputs already apply the camera's effect capability and the Darkness setting.
    next.night_vision = lighting.0.night_vision;
    next.darkness = lighting.0.darkness;
    next.blindness = server_camera
        .as_deref()
        .map_or(*vision, |camera| vision.for_camera(camera))
        .blindness;

    let hud = ui.hud();
    if let Some(health) = hud
        .health()
        .and_then(|stat| fraction(stat.current(), stat.maximum()))
    {
        next.player_health = health;
    }
    if let Some(hunger) = hud
        .hunger()
        .and_then(|stat| fraction(stat.current(), stat.maximum()))
    {
        next.player_hunger = hunger;
    }
    if let Some(air) = ui
        .gameplay_hud()
        .air_ticks()
        .and_then(|(air, maximum)| fraction(air, maximum))
    {
        next.player_air = air;
    }

    let physics = physics.as_deref();
    let (sneaking, sprinting) = physics
        .and_then(LocalPhysicsController::latest_sneak_sprint)
        .unwrap_or_default();
    next.sneaking = sneaking;
    next.sprinting = sprinting;

    if let Some(stream) = client_world.stream.as_ref() {
        let camera = cameras.single().ok().copied();
        let camera_position = camera.map(|transform| transform.translation.to_array());
        let eye = physics
            .and_then(|physics| physics.render_eye_position())
            .or(camera_position);
        let feet = physics
            .and_then(|physics| physics.render_feet_position())
            .or(camera_position);
        publish_world_inputs(&mut next, stream, eye, feet);
        if let Some(camera) = camera {
            publish_camera_inputs(&mut next, stream, &camera);
        }
        if let Some(actor) = stream.authority().actor(stream.local_player_runtime_id()) {
            next.hurt = actor.status.hurt_time > 0;
            next.on_fire = actor.is_on_fire();
            if let Some(on_ground) = actor.on_ground {
                next.on_ground = on_ground;
            }
        }
        if let Some(state) = physics.and_then(|physics| physics.state()) {
            next.on_ground = state.on_ground;
        }
        let main_hand = player_runtime.selected_stack();
        next.held_item_id = held_item_id(stream, &ids, main_hand);
        next.held_block_light = held_block_light(stream, main_hand);
        let off_hand = ui.gameplay_hud().offhand_stack();
        next.held_item_id2 = held_item_id(stream, &ids, off_hand);
        next.held_block_light2 = held_block_light(stream, off_hand);
    }

    inputs.set_if_neq(next);
}

/// Fills the values sampled from the world around the player: light at the eye, the biome at
/// the feet, and the render distance.
fn publish_world_inputs(
    next: &mut ShaderPackFrameInputs,
    stream: &WorldStream,
    eye: Option<[f32; 3]>,
    feet: Option<[f32; 3]>,
) {
    next.render_distance = stream.render_distance_blocks();
    if let Some(range) = stream.dimension_range(stream.current_dimension()) {
        next.bedrock_level = range.base_sub_chunk_y * world::SUB_CHUNK_SIDE as i32;
    }
    if let Some(light) = eye.and_then(|eye| stream.solved_light_at(eye)) {
        next.eye_brightness = eye_brightness(light);
    }
    let Some(feet) = feet else {
        return;
    };
    next.biome = stream.camera_biome_id(feet);
    next.biome_block_y = feet[1].floor() as i32;
}

/// Fills the values sampled along and around the rendered camera: the biomes Java blends
/// the sky and fog colours over, and the distance to the block at the screen's centre.
fn publish_camera_inputs(
    next: &mut ShaderPackFrameInputs,
    stream: &WorldStream,
    camera: &Transform,
) {
    next.sky_biomes = sky_biomes(camera.translation.to_array(), |block| {
        stream.camera_biome_id(block.map(|v| v as f32))
    });
    let forward = camera.rotation * Vec3::NEG_Z;
    next.center_distance = stream.first_block_distance(
        camera.translation.to_array(),
        forward.to_array(),
        next.render_distance,
    );
}

/// The biomes around `position` with Java's Gaussian blend weights, heaviest first and
/// normalised over the kept slots; `biome_at` reads the biome at a block.
fn sky_biomes(
    position: [f32; 3],
    biome_at: impl Fn([i32; 3]) -> Option<u32>,
) -> [(u32, f32); SKY_BIOME_SLOTS] {
    if !position.iter().all(|value| value.is_finite()) {
        return [(0, 0.0); SKY_BIOME_SLOTS];
    }
    // The kernel centres on the position shifted back half a quart, in quarts.
    let axes = position.map(|v| {
        let quart = (v - QUART_BLOCKS as f32 * 0.5) / QUART_BLOCKS as f32;
        let base = quart.floor();
        let fraction = quart - base;
        let weights: [f32; BIOME_BLEND_SPAN] = std::array::from_fn(|i| {
            BIOME_BLEND_KERNEL[i + 1]
                + fraction * (BIOME_BLEND_KERNEL[i] - BIOME_BLEND_KERNEL[i + 1])
        });
        (base as i64, weights)
    });
    let mut totals: Vec<(u32, f32)> = Vec::new();
    for x in 0..BIOME_BLEND_SPAN {
        for y in 0..BIOME_BLEND_SPAN {
            for z in 0..BIOME_BLEND_SPAN {
                let quart = [x, y, z];
                let weight: f32 = (0..3).map(|axis| axes[axis].1[quart[axis]]).product();
                if weight <= 0.0 {
                    continue;
                }
                let block: [Option<i32>; 3] = std::array::from_fn(|axis| {
                    axes[axis]
                        .0
                        .checked_add(quart[axis] as i64 - 2)?
                        .checked_mul(i64::from(QUART_BLOCKS))?
                        .try_into()
                        .ok()
                });
                let [Some(x), Some(y), Some(z)] = block else {
                    continue;
                };
                let Some(biome) = biome_at([x, y, z]) else {
                    continue;
                };
                match totals.iter_mut().find(|(id, _)| *id == biome) {
                    Some((_, total)) => *total += weight,
                    None => totals.push((biome, weight)),
                }
            }
        }
    }
    totals.sort_by(|a, b| b.1.total_cmp(&a.1));
    totals.truncate(SKY_BIOME_SLOTS);
    let sum: f32 = totals.iter().map(|(_, w)| w).sum();
    let mut slots = [(0, 0.0); SKY_BIOME_SLOTS];
    for (slot, (biome, weight)) in slots.iter_mut().zip(totals) {
        *slot = (biome, weight / sum);
    }
    slots
}

/// The pack's `entityId` of every actor the rig renders this frame, by runtime id.
fn publish_actor_ids(
    ids: Res<ShaderPackIdMaps>,
    frame: Res<ActorRenderFrame>,
    client_world: Res<ClientWorld>,
    mut actor_ids: ResMut<ShaderPackActorIds>,
) {
    let mut next = ShaderPackActorIds::default();
    if let (true, Some(stream)) = (ids.is_loaded(), client_world.stream.as_ref()) {
        for entry in frame.rig.manifest.iter() {
            let runtime_id = entry.identity.runtime_id;
            let Some(actor) = stream.authority().actor(runtime_id) else {
                continue;
            };
            let identifier = match &actor.kind {
                protocol::ActorKind::Player { .. } => {
                    assets::facts::VanillaEntity::Player.identifier()
                }
                protocol::ActorKind::Entity { identifier } => identifier,
            };
            if let Some(id) = ids.entity_id(identifier) {
                next.by_runtime_id.insert(runtime_id, id);
            }
        }
    }
    actor_ids.set_if_neq(next);
}

/// `heldItemId` for a held stack: the pack's `item.properties` id of its Bedrock item through
/// the Java mapping, -1 when the hand is empty or the item is unmapped.
fn held_item_id(
    stream: &WorldStream,
    ids: &ShaderPackIdMaps,
    stack: Option<&NetworkItemStack>,
) -> i32 {
    stack
        .and_then(|stack| stream.authority().item_identifier(stack.network_id))
        .and_then(|identifier| ids.item_id(&identifier))
        .unwrap_or(-1)
}

/// Light the held stack's block emits, `0..=15`; 0 for empty hands, items that place no block
/// (Bedrock sends block runtime id 0 for those) and states missing from the block registry.
fn held_block_light(stream: &WorldStream, stack: Option<&NetworkItemStack>) -> i32 {
    stack
        .and_then(|stack| {
            client_interaction::block_use::held_block_store_id(stream, stack.block_runtime_id)
        })
        .and_then(|runtime_id| {
            stream
                .runtime_assets()
                .resolve_known(stream.network_id_mode(), runtime_id)
        })
        .map_or(0, |block| i32::from(block.light_properties().emission()))
}

/// The pack-visible clock derived from one absolute Bedrock tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DayClock {
    /// Tick within the current day, `0..24000`.
    world_time: i32,
    /// Whole days elapsed, saturated to `i32`.
    world_day: i32,
    /// Moon phase `0..8`.
    moon_phase: i32,
    /// Fraction of a turn with 0 at noon, as Java's `getTimeOfDay`.
    celestial_angle: f32,
}

impl DayClock {
    /// Splits an absolute tick into time of day, day count, moon phase and celestial angle.
    /// Non-finite ticks read as tick 0; negative ticks count back from day 0.
    fn at(absolute_ticks: f64) -> Self {
        let ticks = if absolute_ticks.is_finite() {
            absolute_ticks.floor() as i64
        } else {
            0
        };
        let day_ticks = i64::from(world::TICKS_PER_DAY);
        let day = ticks.div_euclid(day_ticks);
        Self {
            world_time: ticks.rem_euclid(day_ticks) as i32,
            world_day: day.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            moon_phase: day.rem_euclid(MOON_PHASES) as i32,
            celestial_angle: celestial::celestial_angle(absolute_ticks),
        }
    }
}

/// Iris `eyeBrightness` from `(block, sky)` light levels: each level times 16, at most 240.
fn eye_brightness((block, sky): (u8, u8)) -> [i32; 2] {
    [block, sky].map(|level| i32::from(level.min(MAX_LIGHT_LEVEL)) * EYE_BRIGHTNESS_PER_LEVEL)
}

/// Iris `isEyeInWater`: 0 air, 1 water, 2 lava.
fn eye_medium(medium: CameraMedium) -> i32 {
    match medium {
        CameraMedium::Air => 0,
        CameraMedium::Water => 1,
        CameraMedium::Lava => 2,
    }
}

/// `current / maximum` clamped to `0..=1`; `None` without a positive maximum.
fn fraction(current: u16, maximum: u16) -> Option<f32> {
    (maximum > 0).then(|| (f32::from(current) / f32::from(maximum)).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds two emissive states with distinct wire and store identities.
    fn held_light_stream(hashed: bool) -> WorldStream {
        use assets::*;
        let visual = BlockVisual::diagnostic(BlockFlags::CUBE_GEOMETRY, ContributorRole::Primary);
        let compiled = CompiledAssets {
            visuals: vec![visual; 3].into_boxed_slice(),
            light_properties: [0, 7, 15]
                .map(|v| LightProperties::new(v, 0).unwrap())
                .into(),
            hashed: vec![(1, 0), (2, 1), (0x8000_0007, 2)].into_boxed_slice(),
            materials: vec![Material {
                texture: TextureRef::DIAGNOSTIC,
                flags: 0,
                animation: NO_ANIMATION,
                ..Material::unvaried()
            }]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![TexturePage::new(TextureArray {
                layers: 1,
                mips: [16u32, 8, 4, 2, 1]
                    .map(|size| TextureMip {
                        size,
                        rgba8: vec![0; size as usize * size as usize * 4].into_boxed_slice(),
                    })
                    .into(),
            })]
            .into_boxed_slice(),
            biomes: CompiledBiomeAssets::diagnostic(),
            provenance: *pinned_world_provenance(),
        };
        let runtime = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
        let mut stream = WorldStream::new_with_assets(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashed,
            },
            std::sync::Arc::new(runtime),
            [0.; 3],
            None,
        );
        stream.set_sequential_id_remap(SequentialIdRemap::from_palette(vec![0, 2, 1], 3));
        stream
    }

    #[test]
    fn held_light_uses_hash_bits_and_the_session_palette() {
        for hashed in [false, true] {
            let stream = held_light_stream(hashed);
            let stack = NetworkItemStack {
                network_id: 1,
                metadata: 0,
                stack_network_id: 1,
                count: 1,
                block_runtime_id: if hashed { 0x8000_0007u32 as i32 } else { 1 },
                nbt_digest: [0; 32],
                extra_data: std::sync::Arc::from([]),
            };
            assert_eq!(
                held_block_light(&stream, Some(&stack)),
                15,
                "hashed={hashed}"
            );
            assert_eq!(held_block_light(&stream, None), 0);
            assert_eq!(
                held_block_light(
                    &stream,
                    Some(&NetworkItemStack {
                        block_runtime_id: 0,
                        ..stack
                    })
                ),
                0
            );
        }
    }

    /// Java's `getTimeOfDay` for a tick, written out independently of the celestial helper.
    fn java_time_of_day(ticks: f64) -> f32 {
        let linear = ((ticks / 24_000.0 - 0.25).rem_euclid(1.0)) as f32;
        let eased = 0.5 - (linear * std::f32::consts::PI).cos() / 2.0;
        (linear * 2.0 + eased) / 3.0
    }

    #[test]
    fn celestial_angle_follows_java_with_noon_at_zero() {
        assert_eq!(DayClock::at(6_000.0).celestial_angle, 0.0);
        assert!((DayClock::at(18_000.0).celestial_angle - 0.5).abs() < 1e-6);
        for ticks in [0.0, 1_000.0, 12_000.0, 13_000.0, 23_999.0, 30_000.0] {
            let angle = DayClock::at(ticks).celestial_angle;
            assert!(
                (angle - java_time_of_day(ticks)).abs() < 1e-5,
                "tick {ticks}: {angle}"
            );
        }
    }

    #[test]
    fn day_clock_splits_time_day_and_moon_phase() {
        let clock = DayClock::at(9.0 * 24_000.0 + 100.5);
        assert_eq!(
            (clock.world_time, clock.world_day, clock.moon_phase),
            (100, 9, 1)
        );
        let midnight = DayClock::at(18_000.0);
        assert_eq!(
            (midnight.world_time, midnight.world_day, midnight.moon_phase),
            (18_000, 0, 0)
        );
    }

    #[test]
    fn day_clock_counts_negative_ticks_back_from_day_zero() {
        let clock = DayClock::at(-1.0);
        assert_eq!(
            (clock.world_time, clock.world_day, clock.moon_phase),
            (23_999, -1, 7)
        );
    }

    #[test]
    fn day_clock_reads_non_finite_ticks_as_tick_zero() {
        for ticks in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(DayClock::at(ticks), DayClock::at(0.0));
        }
    }

    #[test]
    fn day_count_saturates_instead_of_wrapping() {
        assert_eq!(DayClock::at(1e300).world_day, i32::MAX);
        assert_eq!(DayClock::at(-1e300).world_day, i32::MIN);
    }

    #[test]
    fn eye_brightness_scales_each_level_by_sixteen() {
        assert_eq!(eye_brightness((0, 15)), [0, 240]);
        assert_eq!(eye_brightness((7, 0)), [112, 0]);
        assert_eq!(eye_brightness((15, 15)), [240, 240]);
        assert_eq!(eye_brightness((200, 16)), [240, 240]);
    }

    #[test]
    fn eye_medium_uses_iris_codes() {
        assert_eq!(eye_medium(CameraMedium::Air), 0);
        assert_eq!(eye_medium(CameraMedium::Water), 1);
        assert_eq!(eye_medium(CameraMedium::Lava), 2);
    }

    #[test]
    fn fraction_is_bounded_and_needs_a_positive_maximum() {
        assert_eq!(fraction(10, 20), Some(0.5));
        assert_eq!(fraction(20, 20), Some(1.0));
        assert_eq!(fraction(0, 20), Some(0.0));
        assert_eq!(fraction(30, 20), Some(1.0));
        assert_eq!(fraction(1, 0), None);
    }

    #[test]
    fn sky_biomes_skip_unrepresentable_cells() {
        for axis in 0..3 {
            for coordinate in [i32::MIN as f32, i32::MAX as f32] {
                let mut position = [0.0; 3];
                position[axis] = coordinate;
                let samples = sky_biomes(position, |block| {
                    assert_eq!(block[axis].is_negative(), coordinate.is_sign_negative());
                    Some(7)
                });
                assert_eq!(samples[0], (7, 1.0));
            }
            for coordinate in [
                f32::MIN,
                f32::MAX,
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ] {
                let mut position = [0.0; 3];
                position[axis] = coordinate;
                assert_eq!(
                    sky_biomes(position, |_| panic!("unavailable cell")),
                    [(0, 0.0); SKY_BIOME_SLOTS]
                );
            }
        }
    }

    #[test]
    fn sky_biomes_weigh_the_quarts_around_the_camera_like_java() {
        // One biome everywhere: all weight on it.
        let uniform = sky_biomes([8.5, 74.0, 8.5], |_| Some(7));
        assert_eq!(uniform[0], (7, 1.0));
        assert!(uniform[1..].iter().all(|(_, w)| *w == 0.0));
        // A border at x = 8: the kernel spans quarts -1..=4 along x with weights
        // [0.375, 2.125, 4.75, 5.25, 2.875, 0.625] at the camera's x of 8.5, and the
        // quarts from x = 8 on hold 8.75 of their 16.
        let split = sky_biomes([8.5, 74.0, 8.5], |[x, ..]| Some(u32::from(x >= 8)));
        assert_eq!(split[0].0, 1);
        assert!((split[0].1 - 8.75 / 16.0).abs() < 1e-5, "{split:?}");
        assert!((split[1].1 - 7.25 / 16.0).abs() < 1e-5, "{split:?}");
        // Unstreamed biomes drop out; nothing streamed leaves every slot empty.
        assert!(
            sky_biomes([0.0; 3], |_| None)
                .iter()
                .all(|(_, w)| *w == 0.0)
        );
    }
}
