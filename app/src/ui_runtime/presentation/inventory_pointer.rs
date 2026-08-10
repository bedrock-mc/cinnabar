use ui::UiPoint;

use super::{HudGeometry, UiPresentationRuntime};

const PANEL_SIZE: [f32; 2] = [176.0, 166.0];
const SLOT_SIZE: f32 = 18.0;

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

    pub(crate) fn inventory_slot_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
    ) -> Option<u8> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        slot_hit(gui, geometry)
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

fn slot_hit(point: [f32; 2], geometry: HudGeometry) -> Option<u8> {
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
                return Some(9 + row * 9 + column);
            }
        }
    }
    for column in 0..9u8 {
        let min = [
            origin[0] + 8.0 + f32::from(column) * SLOT_SIZE,
            origin[1] + 142.0,
        ];
        if point_in_slot(point, min) {
            return Some(column);
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
}
