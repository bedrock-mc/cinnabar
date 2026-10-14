//! The selection inbox keeps what a replaced walk request carried.

use lod::select::View;
use lod_store::identity::Dimension;

use super::{SelectionInbox, WalkRequest};

/// A request from `x`, resetting hysteresis when `reset`, with `resident` columns.
fn request(x: f64, reset: bool, resident: Option<Vec<[i32; 2]>>) -> WalkRequest {
    WalkRequest {
        view: View {
            camera: [x, 64.0, 0.0],
            focal_px: 900.0,
            threshold_px: 128.0,
            distance_blocks: 512.0,
            min_block_y: -64,
            max_block_y: 320,
        },
        dimension: Dimension(0),
        reset,
        resident,
    }
}

#[test]
fn a_replaced_walk_keeps_its_reset_and_resident_columns() {
    let mut inbox = SelectionInbox::default();
    inbox.ask_walk(request(0.0, true, Some(vec![[1, 2]])));
    inbox.ask_walk(request(16.0, false, None));
    let walk = inbox.walk.take().expect("a walk");
    assert_eq!(walk.view.camera[0], 16.0, "the newest view");
    assert!(walk.reset, "a teleport still resets hysteresis");
    assert_eq!(walk.resident, Some(vec![[1, 2]]));

    inbox.ask_walk(request(0.0, false, Some(vec![[1, 2]])));
    inbox.ask_walk(request(32.0, false, Some(vec![[3, 4]])));
    let walk = inbox.walk.take().expect("a walk");
    assert!(!walk.reset);
    assert_eq!(walk.resident, Some(vec![[3, 4]]), "newer columns win");
}
