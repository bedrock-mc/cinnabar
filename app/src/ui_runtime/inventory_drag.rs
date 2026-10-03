//! Pointer gesture state for the inventory screens: drag-distribute and
//! double-click gather. Pure so the click timing is testable.

use super::presentation::inventory_pointer::InventoryCellHit;

/// Two primary presses this close on one cell gather the held item.
pub(crate) const DOUBLE_CLICK_MILLIS: u64 = 250;
/// Bounds a drag's remembered cells.
const MAX_DRAG_CELLS: usize = 54;

/// What one frame's pointer input asks the inventory to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PointerAction {
    /// A plain primary click on a cell.
    Click(InventoryCellHit),
    /// A plain secondary click on a cell.
    SecondaryClick(InventoryCellHit),
    /// A primary click with Shift held.
    QuickMove(InventoryCellHit),
    /// A drag over two or more cells with the held stack.
    Distribute {
        cells: Vec<InventoryCellHit>,
        one_each: bool,
    },
    Gather,
}

/// One frame of button edges plus the cell under the pointer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PointerFrame {
    pub primary_pressed: bool,
    pub primary_released: bool,
    pub secondary_pressed: bool,
    pub secondary_released: bool,
    pub shift: bool,
    /// Whether the cursor holds an item.
    pub holding: bool,
    pub hit: Option<InventoryCellHit>,
    pub now_millis: u64,
}

#[derive(Debug, Clone, Default)]
struct Drag {
    secondary: bool,
    cells: Vec<InventoryCellHit>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InventoryPointer {
    drag: Option<Drag>,
    last_primary: Option<(InventoryCellHit, u64)>,
}

fn draggable(hit: InventoryCellHit) -> bool {
    matches!(
        hit,
        InventoryCellHit::Player(_) | InventoryCellHit::Storage(_) | InventoryCellHit::Craft(_)
    )
}

impl InventoryPointer {
    /// Forgets any drag in flight, e.g. when the screen changes.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advances the gesture state by one frame and returns what to dispatch.
    pub(crate) fn step(&mut self, frame: PointerFrame) -> Vec<PointerAction> {
        let mut actions = Vec::new();
        if let (Some(drag), Some(hit)) = (self.drag.as_mut(), frame.hit)
            && draggable(hit)
            && !drag.cells.contains(&hit)
            && drag.cells.len() < MAX_DRAG_CELLS
        {
            drag.cells.push(hit);
        }
        if frame.primary_released || frame.secondary_released {
            let released_primary = frame.primary_released;
            if let Some(drag) = self.drag.take() {
                if released_primary != drag.secondary {
                    if drag.cells.len() >= 2 {
                        actions.push(PointerAction::Distribute {
                            cells: drag.cells,
                            one_each: drag.secondary,
                        });
                    } else if let Some(hit) = drag.cells.first().copied().or(frame.hit) {
                        actions.push(if drag.secondary {
                            PointerAction::SecondaryClick(hit)
                        } else {
                            PointerAction::Click(hit)
                        });
                    }
                } else {
                    self.drag = Some(drag);
                }
            }
        }
        if frame.primary_pressed {
            self.press_primary(frame, &mut actions);
        }
        // When both edges arrive together the primary operation wins.
        if frame.secondary_pressed && !frame.primary_pressed {
            self.drag = None;
            let Some(hit) = frame.hit else {
                return actions;
            };
            if frame.holding && draggable(hit) {
                self.drag = Some(Drag {
                    secondary: true,
                    cells: vec![hit],
                });
            } else {
                actions.push(PointerAction::SecondaryClick(hit));
            }
        }
        actions
    }

    fn press_primary(&mut self, frame: PointerFrame, actions: &mut Vec<PointerAction>) {
        self.drag = None;
        let Some(hit) = frame.hit else {
            return;
        };
        let double = self.last_primary.is_some_and(|(cell, at)| {
            cell == hit && frame.now_millis.saturating_sub(at) <= DOUBLE_CLICK_MILLIS
        });
        self.last_primary = Some((hit, frame.now_millis));
        if frame.shift {
            self.last_primary = None;
            actions.push(PointerAction::QuickMove(hit));
        } else if double && frame.holding && draggable(hit) {
            self.last_primary = None;
            actions.push(PointerAction::Gather);
        } else if frame.holding && draggable(hit) {
            // Held stacks act on release, so a drag can claim the gesture first.
            self.drag = Some(Drag {
                secondary: false,
                cells: vec![hit],
            });
        } else {
            actions.push(PointerAction::Click(hit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: InventoryCellHit = InventoryCellHit::Player(9);
    const B: InventoryCellHit = InventoryCellHit::Player(10);

    fn frame(hit: Option<InventoryCellHit>, now: u64) -> PointerFrame {
        PointerFrame {
            primary_pressed: false,
            primary_released: false,
            secondary_pressed: false,
            secondary_released: false,
            shift: false,
            holding: true,
            hit,
            now_millis: now,
        }
    }

    #[test]
    fn single_cell_release_is_a_click() {
        let mut pointer = InventoryPointer::default();
        let press = PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 0)
        };
        assert!(pointer.step(press).is_empty());
        let release = PointerFrame {
            primary_released: true,
            ..frame(Some(A), 10)
        };
        assert_eq!(pointer.step(release), vec![PointerAction::Click(A)]);
    }

    #[test]
    fn dragging_over_two_cells_distributes() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 0)
        });
        pointer.step(frame(Some(B), 5));
        let actions = pointer.step(PointerFrame {
            primary_released: true,
            ..frame(Some(B), 10)
        });
        assert_eq!(
            actions,
            vec![PointerAction::Distribute {
                cells: vec![A, B],
                one_each: false
            }]
        );
    }

    #[test]
    fn secondary_drag_places_one_each() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            secondary_pressed: true,
            ..frame(Some(A), 0)
        });
        pointer.step(frame(Some(B), 5));
        let actions = pointer.step(PointerFrame {
            secondary_released: true,
            ..frame(Some(B), 10)
        });
        assert_eq!(
            actions,
            vec![PointerAction::Distribute {
                cells: vec![A, B],
                one_each: true
            }]
        );
    }

    #[test]
    fn quick_second_press_gathers() {
        let mut pointer = InventoryPointer::default();
        let empty_handed = PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        };
        assert_eq!(pointer.step(empty_handed), vec![PointerAction::Click(A)]);
        let second = PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 100)
        };
        assert_eq!(pointer.step(second), vec![PointerAction::Gather]);
        let late = PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 900)
        };
        assert!(pointer.step(late).is_empty());
    }

    #[test]
    fn shift_click_quick_moves() {
        let mut pointer = InventoryPointer::default();
        let press = PointerFrame {
            primary_pressed: true,
            shift: true,
            holding: false,
            ..frame(Some(A), 0)
        };
        assert_eq!(pointer.step(press), vec![PointerAction::QuickMove(A)]);
    }
    #[test]
    fn shift_quick_move_wins_over_a_recent_primary_click() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        });
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                shift: true,
                ..frame(Some(A), 10)
            }),
            vec![PointerAction::QuickMove(A)]
        );
    }

    #[test]
    fn a_new_completed_gesture_discards_the_previous_drag() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            pointer.step(PointerFrame {
                primary_pressed: true,
                ..frame(Some(A), 0)
            });
            let actions = pointer.step(PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                shift: !secondary,
                holding: false,
                ..frame(Some(B), 500)
            });
            assert_eq!(
                actions,
                vec![if secondary {
                    PointerAction::SecondaryClick(B)
                } else {
                    PointerAction::QuickMove(B)
                }]
            );
            assert!(
                pointer
                    .step(PointerFrame {
                        primary_released: true,
                        ..frame(Some(B), 510)
                    })
                    .is_empty()
            );
        }
    }
}
