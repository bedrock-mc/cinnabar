//! Scene description for the menu panorama cube; the GPU side lives in `panorama_render`.

use std::sync::Arc;

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::{PanoramaFaces, PanoramaView};

/// The panorama shader, for hosts that draw it outside the Bevy render graph.
pub const PANORAMA_WGSL: &str = include_str!("panorama.wgsl");

/// The panorama drawn behind the launcher; `view` is `None` while it is hidden.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
pub struct PanoramaScene {
    pub(crate) faces: Option<Arc<PanoramaFaces>>,
    pub(crate) faces_revision: u64,
    pub(crate) view: Option<PanoramaView>,
    game_hidden: bool,
}

impl PanoramaScene {
    /// Scene-stack visibility is independent of whether a panorama texture is available.
    pub fn set_game_visible(&mut self, visible: bool) {
        self.game_hidden = !visible;
    }

    /// Whether world and first-person passes may submit this frame.
    pub(crate) fn game_visible(&self) -> bool {
        !self.game_hidden && self.view.is_none()
    }

    pub fn set_faces(&mut self, faces: Option<Arc<PanoramaFaces>>) {
        self.faces = faces;
        self.faces_revision = self.faces_revision.wrapping_add(1);
    }

    /// Shows the panorama from `view`, or hides it; non-finite views hide it.
    pub fn show(&mut self, view: Option<PanoramaView>) {
        self.view = view.filter(|view| {
            [
                view.yaw_radians,
                view.pitch_radians,
                view.vertical_fov_radians,
                view.aspect,
            ]
            .iter()
            .chain(&view.tint)
            .all(|value| value.is_finite())
                && view.aspect > 0.0
                && view.vertical_fov_radians > 0.0
        });
    }

    #[must_use]
    pub const fn has_faces(&self) -> bool {
        self.faces.is_some()
    }
}

/// World passes queue nothing under a replacement background or an opaque pack screen.
pub(crate) fn world_passes_enabled(scene: Option<bevy::prelude::Res<PanoramaScene>>) -> bool {
    scene.is_none_or(|scene| scene.game_visible())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_policy_suppresses_world_queues_without_any_panorama_faces() {
        use bevy::prelude::*;
        #[derive(Resource, Default)]
        struct Queued(u32);
        let mut app = App::new();
        app.init_resource::<PanoramaScene>()
            .init_resource::<Queued>()
            .add_systems(
                Update,
                (|mut queued: ResMut<Queued>| queued.0 += 1).run_if(world_passes_enabled),
            );
        app.world_mut()
            .resource_mut::<PanoramaScene>()
            .set_game_visible(false);
        app.update();
        assert_eq!(app.world().resource::<Queued>().0, 0);
        app.world_mut()
            .resource_mut::<PanoramaScene>()
            .set_game_visible(true);
        app.update();
        assert_eq!(app.world().resource::<Queued>().0, 1);
        app.world_mut()
            .resource_mut::<PanoramaScene>()
            .show(Some(view(1.0)));
        app.update();
        assert_eq!(app.world().resource::<Queued>().0, 1);
    }

    fn view(aspect: f32) -> PanoramaView {
        PanoramaView {
            yaw_radians: 0.0,
            pitch_radians: 0.0,
            vertical_fov_radians: 1.0,
            aspect,
            tint: [0.0; 4],
        }
    }

    #[test]
    fn faces_must_be_six_equal_squares() {
        let face = || vec![0; 4 * 4 * 4];
        assert!(PanoramaFaces::new(4, std::array::from_fn(|_| face())).is_some());
        let mut faces: [Vec<u8>; 6] = std::array::from_fn(|_| face());
        faces[5].pop();
        assert!(PanoramaFaces::new(4, faces).is_none());
    }

    #[test]
    fn degenerate_views_hide_the_panorama() {
        let mut scene = PanoramaScene::default();
        scene.show(Some(view(1.5)));
        assert!(scene.view.is_some());
        scene.show(Some(view(f32::NAN)));
        assert!(scene.view.is_none());
        scene.show(Some(view(0.0)));
        assert!(scene.view.is_none());
    }
    #[test]
    fn review_render_panorama_rejects_oversized_dimensions_without_overflow() {
        assert!(PanoramaFaces::new(u32::MAX, std::array::from_fn(|_| Vec::new())).is_none());
    }
}
