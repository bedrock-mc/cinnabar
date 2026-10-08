use super::*;
use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, MovementInput, PlayerState, ScenarioWorld,
    Simulator, SurfaceResponse, Vec3,
};

#[test]
fn captured_slab_support_matches_the_authored_model_top_and_remains_stationary() {
    let Some(directory) = std::env::var_os("CINNABAR_CUSTOM_SLAB_FIXTURE_DIR") else {
        eprintln!("SKIP captured slab support: CINNABAR_CUSTOM_SLAB_FIXTURE_DIR is not set");
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let files = ["block.nbt", "bottom.geo.json", "top.geo.json"].map(|name| directory.join(name));
    if let Some(missing) = files.iter().find(|path| !path.is_file()) {
        eprintln!(
            "SKIP captured slab support: missing fixture {}",
            missing.display()
        );
        return;
    }
    let bytes = std::fs::read(&files[0]).unwrap();
    let mut blocks = CustomBlocks::from_definitions([("test:captured_slab", bytes.as_slice())]);
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 0);
    let block = &blocks.blocks[0];
    assert_eq!(block.state_physics.len(), block.state_count as usize);
    for (state, path) in [(0, &files[1]), (1, &files[2])] {
        let model: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let model = &model["minecraft:geometry"][0];
        let rendered_top = model["bones"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|bone| {
                assert!(
                    bone.get("parent").is_none(),
                    "fixture must have unparented cubes"
                );
                assert!(
                    bone.get("rotation").is_none(),
                    "fixture must have unrotated cubes"
                );
                bone["cubes"].as_array().unwrap()
            })
            .map(|cube| {
                assert!(
                    cube.get("rotation").is_none(),
                    "fixture must have unrotated cubes"
                );
                (cube["origin"][1].as_f64().unwrap() + cube["size"][1].as_f64().unwrap()) / 16.0
            })
            .reduce(f64::max)
            .unwrap();
        let physics = block.physics_for_state(state);
        let shapes = physics.collision_boxes.unwrap();
        assert!(physics.collides);
        let support = shapes
            .iter()
            .map(|shape| f64::from(shape.max[1]))
            .reduce(f64::max)
            .unwrap();
        assert_eq!(support, rendered_top, "state {state}");
        let world = ScenarioWorld {
            name: "custom_slab_support".into(),
            origin: [0; 3],
            revision: 1,
            boxes: shapes
                .iter()
                .map(|shape| {
                    Aabb::new(
                        Vec3::new(
                            f64::from(shape.min[0]),
                            f64::from(shape.min[1]),
                            f64::from(shape.min[2]),
                        ),
                        Vec3::new(
                            f64::from(shape.max[0]),
                            f64::from(shape.max[1]),
                            f64::from(shape.max[2]),
                        ),
                    )
                })
                .collect(),
            physics: BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                flags: BlockPhysicsFlags::default(),
                fluid_height_blocks: 0.0,
                surface_response: SurfaceResponse::None,
            },
            unloaded: false,
        };
        let mut player = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
        let simulator = Simulator::default();
        for _ in 0..80 {
            simulator
                .tick(&mut player, MovementInput::default(), &world)
                .unwrap();
        }
        assert!(player.on_ground);
        assert!(
            (player.position.y - rendered_top).abs() < 1.0e-7,
            "state {state}: {:?}",
            player.position
        );
        let resting = player.position;
        for _ in 0..20 {
            simulator
                .tick(&mut player, MovementInput::default(), &world)
                .unwrap();
        }
        assert_eq!(player.position, resting);
    }
}
