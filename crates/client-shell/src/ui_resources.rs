//! ECS resources holding the Bevy-free UI authority and its presentation.
use bevy::prelude::{Deref, DerefMut, Resource};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Shared UI authority for menu, gameplay and network systems.
#[derive(Clone, Debug, Resource, Deref, DerefMut)]
pub struct UiRuntimeResource(pub UiRuntime);

/// Retained UI presentation: fonts, icons, screens and the published frame.
#[derive(Resource, Deref, DerefMut)]
pub struct UiPresentationResource(pub UiPresentationRuntime);
