//! The F3 overlay adds strips and text only while lines are set.

use ui::DpiScale;

use super::fixture_font;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::{DebugLines, UiPresentationRuntime};

fn vertex_count(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
) -> usize {
    presentation
        .build(
            player_runtime,
            runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
        .vertices
        .len()
}

#[test]
fn debug_lines_add_geometry_and_clearing_them_restores_the_frame() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let runtime = UiRuntime::new(1);
    let bare = vertex_count(&player_runtime, &mut presentation, &runtime);

    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["0 fps".to_owned(), "XYZ: 0 / 2 / 0".to_owned()],
        right: vec!["Targeted Block: 0, 0, 0".to_owned()],
    }));
    let shown = vertex_count(&player_runtime, &mut presentation, &runtime);
    assert!(shown > bare);

    presentation.set_debug_lines(None);
    assert_eq!(
        vertex_count(&player_runtime, &mut presentation, &runtime),
        bare
    );
}
