//! Scene description for the menu panorama cube; the GPU side lives in `panorama_render`.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::{PanoramaFaces, PanoramaView};

/// The panorama drawn behind the launcher; `view` is `None` while it is hidden.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct PanoramaScene {
    /// The faces' pixels until the renderer has uploaded them; see
    /// [`Self::release_uploaded_faces`].
    pub faces: Option<Arc<PanoramaFaces>>,
    pub faces_revision: u64,
    pub view: Option<PanoramaView>,
    game_hidden: bool,
    has_faces: bool,
    /// One past the faces revision the renderer last uploaded, shared with the render world's copy.
    uploaded: Arc<AtomicU64>,
}

impl PanoramaScene {
    /// Scene-stack visibility is independent of whether a panorama texture is available.
    pub fn set_game_visible(&mut self, visible: bool) {
        self.game_hidden = !visible;
    }

    /// Whether world and first-person passes may submit this frame.
    pub fn game_visible(&self) -> bool {
        !self.game_hidden && self.view.is_none()
    }

    pub fn set_faces(&mut self, faces: Option<Arc<PanoramaFaces>>) {
        self.has_faces = faces.is_some();
        self.faces = faces;
        self.faces_revision = self.faces_revision.wrapping_add(1);
    }

    /// Records that the renderer uploaded the faces of `revision`.
    pub fn mark_uploaded(&self, revision: u64) {
        self.uploaded
            .store(revision.wrapping_add(1), Ordering::Release);
    }

    /// Drops the faces' pixels once the renderer holds them on the GPU; the panorama still shows.
    pub fn release_uploaded_faces(&mut self) {
        if self.faces.is_some()
            && self.uploaded.load(Ordering::Acquire) == self.faces_revision.wrapping_add(1)
        {
            self.faces = None;
        }
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

    /// Whether faces were set, including ones already released after upload.
    #[must_use]
    pub const fn has_faces(&self) -> bool {
        self.has_faces
    }
}

/// World passes queue nothing under a replacement background or an opaque pack screen.
pub fn world_passes_enabled(scene: Option<bevy::prelude::Res<PanoramaScene>>) -> bool {
    scene.is_none_or(|scene| scene.game_visible())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The pixels stay until the renderer uploads that exact revision, then go; the panorama still
    // counts as shown, and newer faces wait for their own upload.
    #[test]
    fn faces_are_released_only_after_their_revision_is_uploaded() {
        let faces = || {
            Some(Arc::new(
                PanoramaFaces::new(1, std::array::from_fn(|_| vec![9; 4])).unwrap(),
            ))
        };
        let mut scene = PanoramaScene::default();
        scene.set_faces(faces());
        scene.release_uploaded_faces();
        assert!(scene.faces.is_some());
        scene.mark_uploaded(scene.faces_revision.wrapping_sub(1));
        scene.release_uploaded_faces();
        assert!(scene.faces.is_some());
        let render_copy = scene.clone();
        render_copy.mark_uploaded(render_copy.faces_revision);
        scene.release_uploaded_faces();
        assert!(scene.faces.is_none());
        assert!(scene.has_faces());
        scene.set_faces(faces());
        scene.release_uploaded_faces();
        assert!(scene.faces.is_some());
    }

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
