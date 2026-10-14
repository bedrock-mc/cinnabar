//! Removes controls for product features that Cinnabar does not expose.

use json_ui::{Catalog, RawControl};

/// Removes store navigation and the global Subscriptions settings controls before resolution.
pub fn apply(catalog: &mut Catalog) {
    for control in catalog.controls_mut() {
        remove_controls(control);
    }
}

/// Prunes disabled feature entries and their spacing without changing other controls' order.
fn remove_controls(control: &mut RawControl) {
    control
        .children
        .retain(|child| match child.owner_ns.as_str() {
            "start" | "pause" => {
                !matches!(
                    child.name.as_str(),
                    "store" | "store_button" | "store_button_disabled"
                ) && !child.base.as_deref().is_some_and(|base| {
                    matches!(
                        base,
                        "start.store_button_stack"
                            | "start.store_button"
                            | "pause.store_button"
                            | "pause.store_button_disabled"
                    )
                })
            }
            "settings" => !matches!(
                child.name.as_str(),
                "view_subscriptions_button"
                    | "view_subscriptions_section"
                    | "view_subscriptions_prerelease_section"
                    | "spacer_09"
            ),
            _ => true,
        });
    for child in &mut control.children {
        remove_controls(child);
    }
}
