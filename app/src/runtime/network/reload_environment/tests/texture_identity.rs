//! Optional texture metadata is part of the identity used by real scene publication.

use super::*;
use bevy::{ecs::system::SystemState, prelude::World};

/// Publishes a prepared atmosphere through the production identity guard in a minimal world.
fn publish(world: &mut World, atmosphere: render_scene::AtmosphereTextureAssets) {
    let mut state = SystemState::<EnvironmentPublication<'_>>::new(world);
    state.get_mut(world).unwrap().publish(PreparedEnvironment {
        atmosphere: Some(PreparedAtmosphere {
            atmosphere,
            weather: Default::default(),
            screen_overlays: None,
        }),
        particles: None,
        dependencies: Default::default(),
    });
    state.apply(world);
}

#[test]
fn equal_pixels_in_different_roles_publish_the_new_texture_and_restore_the_base() {
    let base = atmosphere_base();
    let texture_path = |role| {
        base.runtime()
            .unwrap()
            .textures()
            .iter()
            .find(|texture| texture.role == role)
            .unwrap()
            .source_path
            .as_ref()
    };
    let pixels = skybox_png(2, 2, [12, 34, 56, 255]);
    let sun = prepare_atmosphere(
        &view(&[(texture_path(assets::AtmosphereRole::Sun), &pixels)]),
        &base,
    );
    let moon = prepare_atmosphere(
        &view(&[(texture_path(assets::AtmosphereRole::MoonPhases), &pixels)]),
        &base,
    );
    assert_ne!(sun.identity(), moon.identity());
    assert_ne!(sun.identity(), base.identity());
    let mut world = World::new();
    world.insert_resource(base.clone());
    publish(&mut world, sun.clone());
    assert_eq!(
        world
            .resource::<render_scene::AtmosphereTextureAssets>()
            .runtime()
            .unwrap()
            .textures(),
        sun.runtime().unwrap().textures()
    );
    publish(&mut world, moon.clone());
    let current = world.resource::<render_scene::AtmosphereTextureAssets>();
    assert_eq!(current.identity(), moon.identity());
    assert_eq!(
        current.runtime().unwrap().textures(),
        moon.runtime().unwrap().textures()
    );
    let removed = prepare_atmosphere(&view(&[]), &base);
    publish(&mut world, removed);
    let current = world.resource::<render_scene::AtmosphereTextureAssets>();
    assert_eq!(current.identity(), base.identity());
    assert_eq!(
        current.runtime().unwrap().textures(),
        base.runtime().unwrap().textures()
    );
}

#[test]
fn equal_pixel_bytes_with_different_dimensions_publish_the_new_shape() {
    let base = atmosphere_base();
    let path = &base
        .runtime()
        .unwrap()
        .textures()
        .iter()
        .find(|texture| texture.role == assets::AtmosphereRole::Sun)
        .unwrap()
        .source_path;
    let tall = skybox_png(2, 4, [45, 67, 89, 255]);
    let wide = skybox_png(4, 2, [45, 67, 89, 255]);
    let tall = prepare_atmosphere(&view(&[(path, &tall)]), &base);
    let wide_view = view(&[(path, &wide)]);
    let wide = prepare_atmosphere(&wide_view, &base);
    let texture = |source: &render_scene::AtmosphereTextureAssets| {
        source
            .runtime()
            .unwrap()
            .textures()
            .iter()
            .find(|texture| texture.role == assets::AtmosphereRole::Sun)
            .unwrap()
            .clone()
    };
    assert_eq!(texture(&tall).rgba8, texture(&wide).rgba8);
    assert_ne!(tall.identity(), wide.identity());
    assert_eq!(
        wide.identity(),
        prepare_atmosphere(&wide_view, &base).identity()
    );
    let mut world = World::new();
    world.insert_resource(tall);
    publish(&mut world, wide);
    let current = texture(world.resource::<render_scene::AtmosphereTextureAssets>());
    assert_eq!((current.width, current.height), (4, 2));
}
