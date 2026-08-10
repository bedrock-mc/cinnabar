use ui::UiPoint;

use super::{HudGeometry, UiPresentationRuntime};

const PANEL_SIZE: [f32; 2] = [176.0, 166.0];
const SLOT_SIZE: f32 = 18.0;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum InventoryCellHit {
    Player(u8),
    Storage(u8),
}

impl UiPresentationRuntime {
    pub(crate) fn inventory_gui_point(
        &self,
        point: UiPoint,
        physical_size: [u32; 2],
        dpi_scale: f32,
    ) -> Option<[f32; 2]> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        Some(gui_point(point, geometry, self.safe_area))
    }

    #[cfg(test)]
    pub(crate) fn inventory_slot_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
    ) -> Option<u8> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        cell_hit(gui, geometry, None).and_then(|hit| match hit {
            InventoryCellHit::Player(slot) => Some(slot),
            InventoryCellHit::Storage(_) => None,
        })
    }

    pub(crate) fn inventory_cell_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
        storage_slots: Option<usize>,
    ) -> Option<InventoryCellHit> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        cell_hit(gui, geometry, storage_slots)
    }

    fn inventory_geometry(&self, physical_size: [u32; 2], dpi_scale: f32) -> Option<HudGeometry> {
        HudGeometry::new(
            physical_size,
            dpi_scale,
            self.safe_area,
            self.gui_scale_preference,
        )
    }
}

fn gui_point(point: UiPoint, geometry: HudGeometry, safe_area: ui::SafeArea) -> [f32; 2] {
    [
        (point.x() - safe_area.left()) / geometry.scale,
        (point.y() - safe_area.top()) / geometry.scale,
    ]
}

#[cfg(test)]
fn slot_hit(point: [f32; 2], geometry: HudGeometry) -> Option<u8> {
    cell_hit(point, geometry, None).and_then(|hit| match hit {
        InventoryCellHit::Player(slot) => Some(slot),
        InventoryCellHit::Storage(_) => None,
    })
}

fn cell_hit(
    point: [f32; 2],
    geometry: HudGeometry,
    storage_slots: Option<usize>,
) -> Option<InventoryCellHit> {
    if let Some(count @ (27 | 54)) = storage_slots {
        let rows = count / 9;
        let panel_height = 114.0 + rows as f32 * SLOT_SIZE;
        let origin = [
            ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
            ((geometry.gui_height - panel_height) * 0.5).floor(),
        ];
        for slot in 0..count {
            let min = [
                origin[0] + 8.0 + (slot % 9) as f32 * SLOT_SIZE,
                origin[1] + 18.0 + (slot / 9) as f32 * SLOT_SIZE,
            ];
            if point_in_slot(point, min) {
                return Some(InventoryCellHit::Storage(slot as u8));
            }
        }
        let player_y = origin[1] + 32.0 + rows as f32 * SLOT_SIZE;
        for row in 0..3u8 {
            for column in 0..9u8 {
                if point_in_slot(
                    point,
                    [
                        origin[0] + 8.0 + f32::from(column) * SLOT_SIZE,
                        player_y + f32::from(row) * SLOT_SIZE,
                    ],
                ) {
                    return Some(InventoryCellHit::Player(9 + row * 9 + column));
                }
            }
        }
        for column in 0..9u8 {
            if point_in_slot(
                point,
                [
                    origin[0] + 8.0 + f32::from(column) * SLOT_SIZE,
                    player_y + 58.0,
                ],
            ) {
                return Some(InventoryCellHit::Player(column));
            }
        }
        return None;
    }
    let origin = [
        ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
        ((geometry.gui_height - PANEL_SIZE[1]) * 0.5).floor(),
    ];
    for row in 0..3u8 {
        for column in 0..9u8 {
            let min = [
                origin[0] + 8.0 + f32::from(column) * SLOT_SIZE,
                origin[1] + 84.0 + f32::from(row) * SLOT_SIZE,
            ];
            if point_in_slot(point, min) {
                return Some(InventoryCellHit::Player(9 + row * 9 + column));
            }
        }
    }
    for column in 0..9u8 {
        let min = [
            origin[0] + 8.0 + f32::from(column) * SLOT_SIZE,
            origin[1] + 142.0,
        ];
        if point_in_slot(point, min) {
            return Some(InventoryCellHit::Player(column));
        }
    }
    None
}

fn point_in_slot(point: [f32; 2], min: [f32; 2]) -> bool {
    point[0] >= min[0]
        && point[0] < min[0] + SLOT_SIZE
        && point[1] >= min[1]
        && point[1] < min[1] + SLOT_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::SafeArea;

    fn geometry(physical: [u32; 2], dpi: f32, safe: SafeArea) -> HudGeometry {
        HudGeometry::new(physical, dpi, safe, Some(2)).expect("valid inventory geometry")
    }

    #[test]
    fn dpi_and_safe_area_conversion_retains_pointer_outside_slots() {
        let safe = SafeArea::new(20.0, 10.0, 0.0, 0.0).unwrap();
        let geometry = geometry([1920, 1080], 2.0, safe);
        let point = UiPoint::new(420.0, 210.0).unwrap();
        assert_eq!(gui_point(point, geometry, safe), [400.0, 200.0]);
        assert_eq!(slot_hit([0.0, 0.0], geometry), None);
        assert_eq!(
            slot_hit([geometry.gui_width, geometry.gui_height], geometry),
            None
        );
    }

    #[test]
    fn only_the_36_player_cells_are_interactive() {
        let geometry = geometry([1280, 720], 1.0, SafeArea::ZERO);
        let origin = [
            ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
            ((geometry.gui_height - PANEL_SIZE[1]) * 0.5).floor(),
        ];
        assert_eq!(
            slot_hit([origin[0] + 9.0, origin[1] + 85.0], geometry),
            Some(9)
        );
        assert_eq!(
            slot_hit([origin[0] + 153.0, origin[1] + 143.0], geometry),
            Some(8)
        );
        assert_eq!(
            slot_hit([origin[0] + 9.0, origin[1] + 139.0], geometry),
            None
        );
        assert_eq!(slot_hit([origin[0] + 9.0, origin[1] + 9.0], geometry), None);
        assert_eq!(
            slot_hit([origin[0] + 99.0, origin[1] + 19.0], geometry),
            None
        );
        assert_eq!(
            slot_hit([origin[0] - 1.0, origin[1] + 85.0], geometry),
            None
        );
    }

    #[test]
    fn generic_storage_hit_testing_is_exact_for_27_and_54_cells() {
        let geometry = geometry([1280, 720], 1.0, SafeArea::ZERO);
        for count in [27, 54] {
            let rows = count / 9;
            let panel_height = 114.0 + rows as f32 * SLOT_SIZE;
            let origin = [
                ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
                ((geometry.gui_height - panel_height) * 0.5).floor(),
            ];
            assert_eq!(
                cell_hit([origin[0] + 9.0, origin[1] + 19.0], geometry, Some(count)),
                Some(InventoryCellHit::Storage(0))
            );
            let last = count - 1;
            assert_eq!(
                cell_hit(
                    [
                        origin[0] + 9.0 + (last % 9) as f32 * SLOT_SIZE,
                        origin[1] + 19.0 + (last / 9) as f32 * SLOT_SIZE,
                    ],
                    geometry,
                    Some(count),
                ),
                Some(InventoryCellHit::Storage(last as u8))
            );
            assert_eq!(
                cell_hit(
                    [origin[0] + 9.0, origin[1] + 33.0 + rows as f32 * SLOT_SIZE],
                    geometry,
                    Some(count),
                ),
                Some(InventoryCellHit::Player(9))
            );
        }
    }
}
