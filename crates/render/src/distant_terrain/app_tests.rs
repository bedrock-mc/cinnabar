//! The plugin in a real render app: a handed-over update is built, uploaded, committed and
//! drawn through the render schedule without a validation error.

use std::sync::Arc;

use bevy::{
    prelude::{Transform, Vec3},
    render::view::Msaa,
};
use lod::{
    colour::{FaceColour, MaterialColours, TintClass},
    draw::NeighbourLevels,
    fixtures::blocks,
    geometry::SectionPos,
    select::shown::{DrawUpdate, DrawnSection},
};
use render_api::DistantTerrainQuality;
use render_scene::distant_terrain::{
    DistantTerrainColourTables, DistantTerrainLink, DistantTerrainRenderSettings,
    DistantTerrainRenderStats,
};
use test_time::eventually;

use super::{DistantTerrainRenderPlugin, test_meshes::ground};
use crate::chunk::{chunk_app_with, frame, render_plugin};

#[test]
fn the_plugin_makes_the_main_depth_target_sampleable_for_hi_z() {
    use bevy::{prelude::Camera3d, render::render_resource::TextureUsages};
    let mut app = bevy::app::App::new();
    app.add_plugins(DistantTerrainRenderPlugin);
    let camera = app.world_mut().spawn(Camera3d::default()).id();
    app.update();
    let usages = app
        .world()
        .get::<Camera3d>(camera)
        .expect("the camera")
        .depth_texture_usages;
    assert!(TextureUsages::from(usages).contains(TextureUsages::TEXTURE_BINDING));
}

#[test]
fn a_handed_over_update_commits_and_draws_through_the_render_schedule() {
    let Some(render) = render_plugin(wgpu::Backends::PRIMARY, wgpu::Features::empty()) else {
        eprintln!("skipping distant terrain app: missing native GPU adapter fixture");
        return;
    };
    let camera =
        Transform::from_xyz(16.0, 40.0, -40.0).looking_at(Vec3::new(16.0, 4.0, 16.0), Vec3::Y);
    let (mut app, _) = chunk_app_with(render, Msaa::Off, camera, |app| {
        app.add_plugins(DistantTerrainRenderPlugin);
    });
    let link = app.world().resource::<DistantTerrainLink>().clone();
    let mut materials = MaterialColours::new();
    for _ in 1..=blocks::SAND {
        assert!(materials.push([FaceColour::from_srgb([90; 3], TintClass::None, 0); 6]));
    }
    link.set_colours(DistantTerrainColourTables {
        materials: Arc::new(materials),
        biomes: None,
    });
    link.set_settings(Some(DistantTerrainRenderSettings {
        quality: DistantTerrainQuality::Low,
        distance_chunks: 64,
    }));
    link.reset();
    let pos = SectionPos::new(0, 0, 0, 0).expect("origin");
    link.push_update(DrawUpdate {
        upserts: vec![DrawnSection {
            pos,
            mesh: ground(pos, 4, 0),
            neighbours: NeighbourLevels::NONE,
        }],
        removed: Vec::new(),
        relinked: Vec::new(),
        shown: Arc::from([]),
    });
    eventually("the pipelines to build and the update to commit", || {
        frame(&mut app);
        let stats = link.stats();
        stats.ready && stats.committed_updates == 1
    });
    let stats = link.stats();
    assert_eq!(stats.supported, Some(true));
    assert!(stats.tier.is_some());
    assert_eq!(stats.sections, 1);
    assert!(stats.pool_used_bytes > 0);
    assert!(link.ready_for_update());
    eventually("the counters to come back", || {
        frame(&mut app);
        link.stats().visible_clusters > 0
    });

    // Turning drawing off frees the pool.
    link.set_settings(None);
    frame(&mut app);
    let stats = link.stats();
    assert!(!stats.ready);
    assert_eq!(stats.pool_bytes, 0);

    // Once off and published so, frames leave the counters alone.
    link.publish(DistantTerrainRenderStats {
        sections: 99,
        ..stats
    });
    frame(&mut app);
    assert_eq!(
        link.stats().sections,
        99,
        "an off renderer publishes nothing"
    );
}
