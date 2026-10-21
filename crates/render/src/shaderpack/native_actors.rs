//! Selects native actor layers from the pack geometry actually prepared this frame.

use bevy::{ecs::system::SystemParam, prelude::*};

#[cfg(feature = "shaderpacks")]
use super::{
    actors::{self, ViewActors},
    plugin::ShaderPackViews,
};

/// Reads completed pack preparation when native commands execute, including allocation failures.
#[derive(SystemParam)]
pub(crate) struct NativeActorCoverage<'w> {
    #[cfg(feature = "shaderpacks")]
    views: Option<Res<'w, ShaderPackViews>>,
    #[cfg(feature = "shaderpacks")]
    drawn: Option<Res<'w, render_scene::shaderpack::ShaderPackDrawnCategories>>,
    _marker: std::marker::PhantomData<&'w ()>,
}

impl NativeActorCoverage<'_> {
    /// Checks the source the pack prepared for this view, retaining failed sources natively.
    #[cfg(feature = "shaderpacks")]
    fn replaces(&self, view: Entity, source: impl FnOnce(&ViewActors) -> bool) -> bool {
        self.drawn.as_ref().is_some_and(|drawn| drawn.terrain)
            && self.views.as_ref().is_some_and(|views| {
                views
                    .states
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&view)
                    .is_some_and(|state| !state.actors.allocation_failed && source(&state.actors))
            })
    }

    /// Whether this view's pack prepared the world actor source.
    pub(crate) fn actors(&self, view: Entity) -> bool {
        #[cfg(feature = "shaderpacks")]
        {
            self.replaces(view, |actors| actors.replaces_actors)
        }
        #[cfg(not(feature = "shaderpacks"))]
        {
            let _ = view;
            false
        }
    }

    /// Keeps incompatible or unavailable block-entity sources on their native material path.
    pub(crate) fn blocks(&self, view: Entity) -> bool {
        #[cfg(feature = "shaderpacks")]
        {
            self.replaces(view, |actors| actors.replaces_blocks)
        }
        #[cfg(not(feature = "shaderpacks"))]
        {
            let _ = view;
            false
        }
    }

    /// Static viewmodels and unprepared rigs retain the native hand's private depth.
    pub(crate) fn hand(&self, view: Entity) -> bool {
        #[cfg(feature = "shaderpacks")]
        {
            self.replaces(view, |actors| actors.replaces_hand)
        }
        #[cfg(not(feature = "shaderpacks"))]
        {
            let _ = view;
            false
        }
    }
}

/// Tests hand compatibility without making it a condition of world replacement.
#[cfg(feature = "shaderpacks")]
pub(super) fn hand_supported(
    hand: Option<&render_scene::HandRigScene>,
    viewmodel: Option<&render_scene::ViewmodelScene>,
) -> bool {
    viewmodel.is_none_or(|scene| scene.frame.is_none())
        && actors::compatibility::frame_supported(None, hand, None)
}
