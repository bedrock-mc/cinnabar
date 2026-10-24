//! Keeps developer cursor state inside application input processing.

use bevy::{
    app::MainScheduleOrder,
    ecs::schedule::ScheduleLabel,
    prelude::*,
    window::{CursorOptions, InternalWindowState, PrimaryWindow},
};
use client_state::DrivenInput;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct BeginVirtualCursor;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct EndVirtualCursor;

struct PhysicalState {
    entity: Entity,
    internal: InternalWindowState,
    focused: bool,
    cursor: CursorOptions,
    isolated: bool,
}

#[derive(Resource)]
pub(super) struct Isolation {
    hidden: bool,
    physical: Option<PhysicalState>,
    virtual_position: Option<Vec2>,
}

impl Isolation {
    /// Creates cursor isolation for the explicitly selected developer window mode.
    pub(super) fn new(hidden: bool) -> Self {
        Self {
            hidden,
            physical: None,
            virtual_position: None,
        }
    }

    /// Retains logical synthetic coordinates even while Window hit testing clips them.
    pub(super) fn set_pointer(&mut self, position: Vec2) {
        self.virtual_position = Some(position);
    }
}

/// Installs the virtual cursor's application-frame boundary.
pub(super) fn configure(app: &mut App, hidden: bool) {
    app.insert_resource(Isolation::new(hidden))
        .add_systems(BeginVirtualCursor, begin)
        .add_systems(EndVirtualCursor, end);
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_before(PreUpdate, BeginVirtualCursor);
    // Winit consumes cursor and window changes in Last. This separate schedule also
    // covers every UI writer in PostUpdate, independent of their relative ordering.
    order.insert_before(Last, EndVirtualCursor);
}

/// Saves physical cursor state before application systems receive synthetic input.
fn begin(
    mut isolation: ResMut<Isolation>,
    driven: Option<Res<DrivenInput>>,
    mut windows: Query<(Entity, &mut Window, &CursorOptions), With<PrimaryWindow>>,
) {
    isolation.physical = None;
    if driven.is_none() {
        isolation.virtual_position = None;
    }
    let Ok((entity, mut window, cursor)) = windows.single_mut() else {
        return;
    };
    let isolated = isolation.hidden || driven.is_some();
    isolation.physical = Some(PhysicalState {
        entity,
        internal: window.internal,
        focused: window.focused,
        cursor: cursor.clone(),
        isolated,
    });
    if driven.is_some() {
        window
            .bypass_change_detection()
            .set_cursor_position(isolation.virtual_position);
    }
}

/// Restores physical state before native output, retaining unrelated window requests.
fn end(
    mut isolation: ResMut<Isolation>,
    driven: Option<Res<DrivenInput>>,
    mut windows: Query<(Entity, &mut Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let Some(physical) = isolation.physical.take() else {
        return;
    };
    let Ok((entity, mut window, mut cursor)) = windows.single_mut() else {
        return;
    };
    if entity != physical.entity || !(physical.isolated || driven.is_some()) {
        return;
    }
    if driven.is_none() {
        isolation.virtual_position = None;
    }
    let window = window.bypass_change_detection();
    // Copy the opaque cursor state exactly, including subpixel positions and positions
    // outside the window. Preserve this frame's non-cursor backend requests as well.
    let mut requests = std::mem::replace(&mut window.internal, physical.internal);
    if let Some(maximized) = requests.take_maximize_request() {
        window.set_maximized(maximized);
    }
    if let Some(minimized) = requests.take_minimize_request() {
        window.set_minimized(minimized);
    }
    if requests.take_move_request() {
        window.start_drag_move();
    }
    if let Some(direction) = requests.take_resize_request() {
        window.start_drag_resize(direction);
    }
    window.focused = physical.focused;
    *cursor.bypass_change_detection() = physical.cursor;
    // Restoring equal values alone is insufficient: Winit attempts a grab for every
    // Changed<CursorOptions>, including None.
    client_shell::physical_input::withhold_cursor_change(&mut cursor);
    // Visible driven control must release capture held before isolation began. Once
    // released, this helper leaves later synthetic cursor changes suppressed.
    if !isolation.hidden {
        client_shell::physical_input::release_cursor(&mut cursor);
    }
}

#[cfg(test)]
mod tests;
