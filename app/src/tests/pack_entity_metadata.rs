//! Independently authored server-pack entities driven by actor metadata end to end: the metadata
//! variant selects the render controller's texture and geometry, and the metadata scale sizes
//! the model.
use crate::presentation::{actors, entity_layers};
use assets::{RuntimeAssets, RuntimeEntityAssets};
use chunk_pipeline::WorldStream;
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    ActorSpawnEvent, WorldBootstrap, WorldEvent,
};
use render::ActorArtworkPages;
use render_model::EntityRigId;
use std::sync::Arc;

const COUNTER: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:counter",
 "materials":{"default":"entity_alphatest"},
 "textures":{"zero":"textures/entity/counter_zero","one":"textures/entity/counter_one"},
 "geometry":{"default":"geometry.counter"},
 "render_controllers":["controller.render.counter"]}}}"#;

// No `default` geometry alias: the controller picks the model, as display packs author it.
const HOLOGRAM: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:hologram",
 "materials":{"default":"entity_alphatest"},
 "textures":{"zero":"textures/entity/counter_zero","one":"textures/entity/counter_one"},
 "geometry":{"zero":"geometry.counter","one":"geometry.hologram_one"},
 "render_controllers":["controller.render.hologram"]}}}"#;

// A flipbook: `uv_anim` steps down a four-frame strip with the actor's life time.
const LOGO: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:logo",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "render_controllers":["controller.render.logo"]}}}"#;

// Two controllers, each drawing its own model: the digit's `count` bone exists only there, so
// the clip moving it must still reach that model though the rig poses the background.
const TITLE: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:title",
 "materials":{"default":"entity_alphatest"},
 "textures":{"zero":"textures/entity/counter_zero","one":"textures/entity/counter_one"},
 "geometry":{"digit":"geometry.title_digit","background":"geometry.title_background"},
 "animations":{"center":"animation.title.center"},
 "scripts":{"animate":["center"]},
 "render_controllers":["controller.render.title.digit","controller.render.title.background"]}}}"#;

const TITLE_STILL: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:title_still",
 "materials":{"default":"entity_alphatest"},
 "textures":{"zero":"textures/entity/counter_zero","one":"textures/entity/counter_one"},
 "geometry":{"digit":"geometry.title_digit","background":"geometry.title_background"},
 "render_controllers":["controller.render.title.digit","controller.render.title.background"]}}}"#;

const TITLE_GEOMETRY: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[
 {"description":{"identifier":"geometry.title_digit","texture_width":16,"texture_height":16},"bones":[
 {"name":"root","pivot":[0,4,0]},
 {"name":"count","parent":"root","pivot":[0,0,0],"cubes":[{"origin":[0,3,0],"size":[5,5,0],"uv":[0,0]}]}]},
 {"description":{"identifier":"geometry.title_background","texture_width":16,"texture_height":16},"bones":[
 {"name":"root","pivot":[0,10,0],"cubes":[{"origin":[-8,0,0],"size":[16,16,0],"uv":[0,0]}]}]}]}"#;

const TITLE_ANIMATION: &str = r#"{"format_version":"1.8.0","animations":{
 "animation.title.center":{"loop":true,"bones":{"count":{"position":[3,0,0]}}}}}"#;

const TITLE_RENDER: &str = r#"{"format_version":"1.8.0","render_controllers":{
 "controller.render.title.digit":{"geometry":"Geometry.digit","materials":[{"*":"Material.default"}],
  "textures":["Texture.one"]},
 "controller.render.title.background":{"geometry":"Geometry.background",
  "materials":[{"*":"Material.default"}],"textures":["Texture.zero"]}}}"#;

// A one-shot lift and a held one on the same model.
const ONCE: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:once",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "animations":{"lift":"animation.test.lift_once","hold":"animation.test.lift_hold"},
 "scripts":{"animate":["lift"]},
 "render_controllers":["controller.render.logo"]}}}"#;

const HOLD: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:hold",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "animations":{"hold":"animation.test.lift_hold"},
 "scripts":{"animate":["hold"]},
 "render_controllers":["controller.render.logo"]}}}"#;

const LIFT_ANIMATION: &str = r#"{"format_version":"1.8.0","animations":{
 "animation.test.lift_once":{"animation_length":0.25,"bones":{"root":{"position":[0,8,0]}}},
 "animation.test.lift_hold":{"loop":"hold_on_last_frame","animation_length":0.25,
  "bones":{"root":{"position":[0,8,0]}}}}}"#;

// Scale scripts as vanilla mobs author them: a Molang uniform scale and a per-axis squash.
const SCALED: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:scaled",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "scripts":{"scale":"query.variant == 1 ? 2.0 : 1.0","scaleY":"0.5"},
 "render_controllers":["controller.render.logo"]}}}"#;

// An override clip rotating a bone clears the translation an earlier clip gave it.
const OVERRIDE: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:override",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "animations":{"lift":"animation.test.lift_loop","turn":"animation.test.turn_override"},
 "scripts":{"animate":["lift","turn"]},
 "render_controllers":["controller.render.logo"]}}}"#;

// The same clips authored override-first: the later lift survives the earlier override.
const ORDERED: &str = r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
 "identifier":"test:ordered",
 "materials":{"default":"entity_alphatest"},
 "textures":{"default":"textures/entity/counter_zero"},
 "geometry":{"default":"geometry.counter"},
 "animations":{"lift":"animation.test.lift_loop","turn":"animation.test.turn_override"},
 "scripts":{"animate":["turn","lift"]},
 "render_controllers":["controller.render.logo"]}}}"#;

const OVERRIDE_ANIMATION: &str = r#"{"format_version":"1.8.0","animations":{
 "animation.test.lift_loop":{"loop":true,"bones":{"root":{"position":[0,8,0]}}},
 "animation.test.turn_override":{"loop":true,"override_previous_animation":true,
  "bones":{"root":{"rotation":[0,90,0]}}}}}"#;

const GEOMETRY: &str = r#"{"format_version":"1.12.0","minecraft:geometry":[
 {"description":{"identifier":"geometry.counter","texture_width":16,"texture_height":16},"bones":[
 {"name":"root","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-4],"size":[8,16,8],"uv":[0,0]}]}]},
 {"description":{"identifier":"geometry.hologram_one","texture_width":16,"texture_height":16},"bones":[
 {"name":"root","pivot":[0,0,0],"cubes":[{"origin":[-2,0,-2],"size":[4,24,4],"uv":[0,0]}]}]}]}"#;

const RENDER: &str = r#"{"format_version":"1.8.0","render_controllers":{
 "controller.render.counter":{
 "arrays":{"textures":{"Array.digits":["Texture.zero","Texture.one"]}},
 "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
 "textures":["Array.digits[query.variant]"]},
 "controller.render.hologram":{
 "arrays":{"textures":{"Array.digits":["Texture.zero","Texture.one"]},
  "geometries":{"Array.models":["Geometry.zero","Geometry.one"]}},
 "geometry":"Array.models[query.variant]","materials":[{"*":"Material.default"}],
 "textures":["Array.digits[query.variant]"]},
 "controller.render.logo":{
 "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
 "textures":["Texture.default"],"ignore_lighting":true,
 "uv_anim":{"offset":[0.0,"math.mod(math.floor(query.life_time * 120), 4) / 4"],
  "scale":[1.0,"1 / 4"]}}}}"#;

fn png(colour: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba(colour))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

type Pack = (Arc<RuntimeEntityAssets>, Vec<u32>);

fn pack() -> (Pack, ActorArtworkPages) {
    let files: Vec<(Box<str>, Vec<u8>)> = vec![
        ("entity/counter.entity.json".into(), COUNTER.into()),
        ("entity/hologram.entity.json".into(), HOLOGRAM.into()),
        ("entity/logo.entity.json".into(), LOGO.into()),
        ("entity/title.entity.json".into(), TITLE.into()),
        ("entity/once.entity.json".into(), ONCE.into()),
        ("entity/override.entity.json".into(), OVERRIDE.into()),
        ("entity/ordered.entity.json".into(), ORDERED.into()),
        (
            "animations/override.animation.json".into(),
            OVERRIDE_ANIMATION.into(),
        ),
        ("entity/scaled.entity.json".into(), SCALED.into()),
        ("entity/hold.entity.json".into(), HOLD.into()),
        (
            "animations/lift.animation.json".into(),
            LIFT_ANIMATION.into(),
        ),
        ("entity/title_still.entity.json".into(), TITLE_STILL.into()),
        ("models/entity/title.geo.json".into(), TITLE_GEOMETRY.into()),
        (
            "animations/title.animation.json".into(),
            TITLE_ANIMATION.into(),
        ),
        (
            "render_controllers/title.render_controllers.json".into(),
            TITLE_RENDER.into(),
        ),
        ("models/entity/counter.geo.json".into(), GEOMETRY.into()),
        (
            "render_controllers/counter.render_controllers.json".into(),
            RENDER.into(),
        ),
        (
            "textures/entity/counter_zero.png".into(),
            png([10, 0, 0, 255]),
        ),
        (
            "textures/entity/counter_one.png".into(),
            png([0, 10, 0, 255]),
        ),
    ];
    let compiled = pack_compiler::compile_actor_pack(files).unwrap().unwrap();
    let artwork =
        ActorArtworkPages::default().with_pack_artwork(&compiled.textures, &compiled.bindings);
    let candidates = compiled
        .bindings
        .iter()
        .map(|binding| binding.geometry_candidate)
        .collect();
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    ((assets, candidates), artwork)
}

fn metadata(sequence: u64, key: u32, value: ActorMetadataValue) -> (u64, WorldEvent) {
    (
        sequence,
        WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 42,
            metadata: Arc::from([ActorMetadata { key, value }]),
            properties: Arc::from([]),
            tick: sequence,
        })),
    )
}

fn world(pack: Pack, identifier: &str) -> WorldStream {
    let mut world = WorldStream::new_with_asset_sets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        // The pack doubles as the base catalog; its layer still wins and yields pack rig ids.
        Arc::clone(&pack.0),
        [0.0, 64.0, 0.0],
        None,
    );
    world.set_pack_entities(Some(pack));
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
                metadata: Arc::from([ActorMetadata {
                    key: 2,
                    value: ActorMetadataValue::Int(0),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    world.advance_actor_interpolation_ticks(2);
    world
}

fn update(world: &mut WorldStream, key: u32, value: ActorMetadataValue) {
    let (sequence, event) = metadata(2, key, value);
    world.submit(sequence, event).unwrap();
    world.advance_actor_interpolation_ticks(1);
}

struct Drawn {
    rig: EntityRigId,
    texture_layer: u32,
    /// Length of the drawn model's vertical axis, which the culling box follows.
    height_axis: f32,
    uv_anim: [f32; 4],
}

fn drawn(world: &WorldStream, artwork: &ActorArtworkPages) -> Drawn {
    let rig = world.authority().actor_rig(42).unwrap();
    let body =
        actors::entity_rig_presentation(&rig, world.authority().actor(42).unwrap(), artwork, 0.5)
            .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    entity_layers::apply_render_layers(&mut batch, |id| world.authority().actor_rig(id), artwork);
    let submission = &batch.submissions[0];
    let matrix = submission.world_from_actor;
    Drawn {
        rig: submission.input.rig,
        texture_layer: submission.texture_layer,
        height_axis: (0..3).map(|row| matrix[row][1].powi(2)).sum::<f32>().sqrt(),
        uv_anim: submission.uv_anim,
    }
}

// A server-pack render controller picks its texture from the metadata variant every tick.
#[test]
fn metadata_variant_selects_the_pack_render_controller_texture() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:counter");
    let before = drawn(&world, &artwork).texture_layer;
    update(&mut world, 2, ActorMetadataValue::Int(1));
    assert_ne!(
        before,
        drawn(&world, &artwork).texture_layer,
        "variant 1 draws the other texture layer"
    );
}

// Without a `default` alias the pack entity still gets a rig, and the variant swaps its model.
#[test]
fn metadata_variant_selects_the_pack_geometry_without_a_default_alias() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:hologram");
    let zero = drawn(&world, &artwork);
    update(&mut world, 2, ActorMetadataValue::Int(1));
    let one = drawn(&world, &artwork);
    assert_ne!(zero.rig, one.rig, "variant 1 selects the other model");
    assert_ne!(zero.texture_layer, one.texture_layer);
}

// The metadata scale multiplies the model's size and its culling bounds.
#[test]
fn metadata_scale_multiplies_the_rendered_model() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:counter");
    let unscaled = drawn(&world, &artwork);
    update(&mut world, 38, ActorMetadataValue::Float(2.0));
    let scaled = drawn(&world, &artwork);
    assert!((scaled.height_axis - unscaled.height_axis * 2.0).abs() < 1e-5);
}

// `uv_anim` reaches the draw: the scale picks one frame and the offset follows the life time.
#[test]
fn render_controller_uv_anim_steps_the_flipbook_frame() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:logo");
    let first = drawn(&world, &artwork).uv_anim;
    assert_eq!([first[0], first[2], first[3]], [0.0, 1.0, 0.25]);
    world.advance_actor_interpolation_ticks(1);
    let second = drawn(&world, &artwork).uv_anim;
    // Six frames pass per tick at 120 frames a second, two past a whole strip of four.
    assert_eq!((second[1] - first[1]).rem_euclid(1.0), 0.5);
}

fn layered(world: &WorldStream, artwork: &ActorArtworkPages) -> actors::ActorPresentationBatch {
    let rig = world.authority().actor_rig(42).unwrap();
    let body =
        actors::entity_rig_presentation(&rig, world.authority().actor(42).unwrap(), artwork, 0.5)
            .unwrap();
    let mut batch = actors::select_actor_presentations(1, false, None, [body]);
    entity_layers::apply_render_layers(&mut batch, |id| world.authority().actor_rig(id), artwork);
    batch
}

fn batch(world: &WorldStream, artwork: &ActorArtworkPages) -> Vec<render::ActorRigSubmission> {
    layered(world, artwork).submissions
}

// Every controller draws with its own geometry and texture; neither is dropped for not matching
// the rig's model.
#[test]
fn each_render_controller_draws_its_own_geometry() {
    let (pack, artwork) = pack();
    let assets = Arc::clone(&pack.0);
    let world = world(pack, "test:title");
    let layered = layered(&world, &artwork);
    let mut scene = render::ActorRenderScene::default();
    scene.replace_pack_entities(Some(&assets)).unwrap();
    scene.configure_artwork(artwork.clone());
    let frame = scene.update_rigs_with_artwork(
        0.5,
        None,
        layered.submissions.clone(),
        &[],
        &layered.artwork,
    );
    assert_eq!(frame.rig.instances.len(), 2, "{:?}", frame.rig.rejects);
    let submissions = layered.submissions;
    assert_eq!(submissions.len(), 2, "both controllers draw");
    let (digit, background) = (&submissions[0], &submissions[1]);
    assert_ne!(digit.input.rig, background.input.rig);
    assert_ne!(digit.texture_layer, background.texture_layer);
    assert_eq!(
        digit.input.current_bones.len(),
        2,
        "the digit model's own bones"
    );
    assert_eq!(background.input.current_bones.len(), 1);
}

// A clip moving a bone only a controller's model has still poses that model.
#[test]
fn clips_pose_bones_only_a_controller_model_has() {
    let (pack, artwork) = pack();
    let count_x = |identifier: &str| {
        let world = world(pack.clone(), identifier);
        batch(&world, &artwork)[0].input.current_bones[1].translation_scale
    };
    let (moved, still) = (count_x("test:title"), count_x("test:title_still"));
    let offset: f32 = (0..3)
        .map(|axis| (moved[axis] - still[axis]).powi(2))
        .sum::<f32>();
    assert!(
        (offset.sqrt() - 3.0 / 16.0).abs() < 1e-4,
        "{moved:?} vs {still:?}"
    );
}

// Render-controller array indices wrap past the end as Molang arrays do: variant 2 of a
// two-texture array draws the first texture again, not the last.
#[test]
fn array_indices_past_the_end_wrap_to_the_first_member() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:counter");
    let zero = drawn(&world, &artwork).texture_layer;
    update(&mut world, 2, ActorMetadataValue::Int(2));
    assert_eq!(drawn(&world, &artwork).texture_layer, zero);
}

// A finished one-shot animation stops posing the model; a hold keeps its last frame.
#[test]
fn a_finished_once_animation_releases_the_pose_while_hold_keeps_it() {
    let (pack, _) = pack();
    let lift = |identifier: &str| {
        let mut world = world(pack.clone(), identifier);
        world.advance_actor_interpolation_ticks(20);
        let rig = world.authority().actor_rig(42).unwrap();
        rig.current[0].translation_scale[1] - rig.rest[0].translation_scale[1]
    };
    assert!(lift("test:once").abs() < 1e-6);
    let held = lift("test:hold");
    assert!((held - 8.0).abs() < 1e-4, "{held}");
}

// A controller with `ignore_lighting` draws unlit even where the world lights the body.
#[test]
fn ignore_lighting_controllers_draw_unlit_while_others_keep_world_light() {
    let (pack, artwork) = pack();
    let light = |identifier: &str| {
        let world = world(pack.clone(), identifier);
        let rig = world.authority().actor_rig(42).unwrap();
        let mut body = actors::entity_rig_presentation(
            &rig,
            world.authority().actor(42).unwrap(),
            &artwork,
            0.5,
        )
        .unwrap();
        body.submission.light = render::pack_actor_light(2, 9);
        let mut batch = actors::select_actor_presentations(1, false, None, [body]);
        entity_layers::apply_render_layers(
            &mut batch,
            |id| world.authority().actor_rig(id),
            &artwork,
        );
        batch.submissions[0].light
    };
    assert_eq!(light("test:logo"), 0);
    assert_eq!(light("test:counter"), render::pack_actor_light(2, 9));
}

// Authored scale expressions and axis scales size the model each tick.
#[test]
fn scale_scripts_size_the_model_per_tick() {
    let (pack, artwork) = pack();
    let mut world = world(pack, "test:scaled");
    let unit = drawn(&world, &artwork);
    assert!(
        (unit.height_axis - 0.5).abs() < 1e-5,
        "{}",
        unit.height_axis
    );
    update(&mut world, 2, ActorMetadataValue::Int(1));
    let doubled = drawn(&world, &artwork);
    assert!(
        (doubled.height_axis - 1.0).abs() < 1e-5,
        "{}",
        doubled.height_axis
    );
}

// Vanilla restores a bone's whole default pose before an override clip, not one channel.
#[test]
fn an_override_clip_resets_the_whole_bone_pose() {
    let (pack, _) = pack();
    let world = world(pack, "test:override");
    let rig = world.authority().actor_rig(42).unwrap();
    let lift = rig.current[0].translation_scale[1] - rig.rest[0].translation_scale[1];
    assert!(lift.abs() < 1e-5, "{lift}");
}

// Root animations run in their authored `animate` order, not sorted by alias.
#[test]
fn root_animations_run_in_authored_order() {
    let (pack, _) = pack();
    let world = world(pack, "test:ordered");
    let rig = world.authority().actor_rig(42).unwrap();
    let lift = rig.current[0].translation_scale[1] - rig.rest[0].translation_scale[1];
    assert!((lift - 8.0).abs() < 1e-4, "{lift}");
}
