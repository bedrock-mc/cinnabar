//! Hidden clients render continuously without acquiring a native window drawable.

use bevy::{
    camera::RenderTarget,
    prelude::*,
    window::{PrimaryWindow, WindowRef},
};
use client_presentation::camera::FlyCamera;

/// The retained output shared by the hidden primary camera and all frame consumers.
#[derive(Resource, Default)]
pub(super) struct FrameTarget(pub(super) Option<Handle<Image>>);

/// Tracks the hidden primary window's physical size before camera projection updates.
pub(super) fn prepare(
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut cameras: Query<&mut RenderTarget, With<FlyCamera>>,
    mut target: ResMut<FrameTarget>,
    images: Option<ResMut<Assets<Image>>>,
) {
    let Ok((window_entity, window)) = windows.single() else {
        return;
    };
    let Some(mut images) = images else { return };
    if window.visible {
        if let Some(image) = target.0.take() {
            for mut camera in &mut cameras {
                if matches!(&*camera, RenderTarget::Image(output) if output.handle == image) {
                    *camera = RenderTarget::Window(WindowRef::Primary);
                }
            }
            images.remove(image.id());
        }
        return;
    }
    let size = UVec2::new(window.physical_width(), window.physical_height());
    if size.x == 0 || size.y == 0 || cameras.is_empty() {
        return;
    }
    let image = target.0.get_or_insert_with(|| {
        images.add(Image::new_target_texture(
            size.x,
            size.y,
            render::SCENE_COLOR_FORMAT,
            None,
        ))
    });
    if let Some(mut output) = images.get_mut(&*image)
        && output.size() != size
    {
        output.resize(bevy::render::render_resource::Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    for mut camera in &mut cameras {
        if matches!(&*camera, RenderTarget::Window(window) if matches!(window, WindowRef::Primary) || matches!(window, WindowRef::Entity(entity) if *entity == window_entity))
        {
            *camera = RenderTarget::Image(image.clone().into());
        }
    }
}
