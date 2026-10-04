use protocol::WindowKind;
use ui::UiPoint;

use super::screens::{self, Widget};
use super::{HudGeometry, UiPresentationRuntime};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryCellHit {
    Player(u8),
    Storage(u8),
    Armor(u8),
    Offhand,
    /// A screen input or crafting cell by UI inventory slot.
    Craft(u8),
    /// The created-output cell of a crafting-style screen.
    CraftOutput,
    Widget(Widget),
    /// A creative catalog grid cell by position on the current page.
    CreativeGrid(u8),
    CreativeTab(u8),
    CreativeSearch,
    /// An entry of the engine-drawn recipe book (the creative catalog in
    /// creative) by position in its list.
    RecipeBook(u16),
}

impl InventoryCellHit {
    /// Whether the hit is a cell holding an item (not a control or tab).
    pub const fn is_item_cell(self) -> bool {
        !matches!(
            self,
            Self::Widget(_) | Self::CreativeTab(_) | Self::CreativeSearch
        )
    }
}

/// Which inventory screen is drawn.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryScreen {
    Personal,
    Workbench,
    Storage(usize),
    /// A container window other than chests and the workbench, with its cell count.
    Window(WindowKind, usize),
    Creative,
    /// A book reader, editor or lectern page.
    Book,
}

impl InventoryScreen {
    pub fn of(ledger: &crate::ui_runtime::inventory_ledger::PlayerInventoryLedger) -> Self {
        let cells = ledger.storage_slot_count();
        match (ledger.window_kind(), cells) {
            (Some(WindowKind::Storage), Some(count))
                if super::forms::supported_storage_slots(count) =>
            {
                Self::Storage(count)
            }
            (Some(WindowKind::Workbench), _) => Self::Workbench,
            (Some(WindowKind::Storage) | None, _) => Self::Personal,
            (Some(kind), cells) => Self::Window(kind, cells.unwrap_or(0)),
        }
    }

    /// The screen for the runtime's state: creative mode swaps the personal
    /// screen for the creative catalog.
    pub fn of_runtime(
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
    ) -> Self {
        if runtime.screen_state().book.is_some() {
            return Self::Book;
        }
        let screen = Self::of(runtime.inventory_ledger(player_runtime));
        let creative = player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| mode == protocol::PlayerGameMode::Creative);
        if screen == Self::Personal
            && creative
            && runtime
                .inventory_ledger(player_runtime)
                .creative_catalog()
                .is_some()
        {
            Self::Creative
        } else {
            screen
        }
    }
}

pub use screens::{WORKBENCH_GRID, WORKBENCH_OUTPUT};

impl UiPresentationRuntime {
    pub fn inventory_gui_point(
        &self,
        point: UiPoint,
        physical_size: [u32; 2],
        dpi_scale: f32,
    ) -> Option<[f32; 2]> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        Some(gui_point(point, geometry, self.safe_area))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn inventory_slot_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
    ) -> Option<u8> {
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        slot_hit(gui, geometry)
    }

    pub fn inventory_cell_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
        screen: InventoryScreen,
    ) -> Option<InventoryCellHit> {
        // An engine-drawn container answers from its own hit regions.
        if self.engine_container_frame().is_some() {
            return self.engine_container_hit(gui);
        }
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        cell_hit(gui, geometry, screen)
    }

    /// The recipe-book control under a GUI point, if the screen has a book.
    pub fn inventory_book_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
        screen: InventoryScreen,
        book_open: bool,
    ) -> Option<InventoryCellHit> {
        // An engine-drawn screen carries its own recipe book controls.
        if self.engine_container_frame().is_some() {
            return None;
        }
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        let origin = screens::panel_origin(screen, [geometry.gui_width, geometry.gui_height]);
        screens::book_hit(screen, origin, gui, book_open).map(InventoryCellHit::Widget)
    }

    /// The book control under a GUI point while a book screen is open.
    pub fn inventory_reader_hit(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
        editable: bool,
        signing: bool,
    ) -> Option<InventoryCellHit> {
        // The vanilla book screen answers from its own hit regions.
        if self.engine_container_frame().is_some() {
            return self.engine_container_hit(gui);
        }
        let geometry = self.inventory_geometry(physical_size, dpi_scale)?;
        let origin = screens::panel_origin(
            InventoryScreen::Book,
            [geometry.gui_width, geometry.gui_height],
        );
        screens::reader_hit([gui[0] - origin[0], gui[1] - origin[1]], editable, signing)
            .map(|button| InventoryCellHit::Widget(Widget::Reader(button)))
    }

    /// Whether a GUI point lies on the drawn inventory panel.
    pub fn inventory_panel_contains(
        &self,
        gui: [f32; 2],
        physical_size: [u32; 2],
        dpi_scale: f32,
        screen: InventoryScreen,
    ) -> bool {
        if let Some(frame) = self.engine_container_frame() {
            return super::forms::engine_panel_contains(frame, gui);
        }
        let Some(geometry) = self.inventory_geometry(physical_size, dpi_scale) else {
            return false;
        };
        let size = screens::panel_size(screen);
        let origin = screens::panel_origin(screen, [geometry.gui_width, geometry.gui_height]);
        gui[0] >= origin[0]
            && gui[0] < origin[0] + size[0]
            && gui[1] >= origin[1]
            && gui[1] < origin[1] + size[1]
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

#[cfg(any(test, feature = "test-support"))]
fn slot_hit(point: [f32; 2], geometry: HudGeometry) -> Option<u8> {
    match cell_hit(point, geometry, InventoryScreen::Personal)? {
        InventoryCellHit::Player(slot) => Some(slot),
        _ => None,
    }
}

fn cell_hit(
    point: [f32; 2],
    geometry: HudGeometry,
    screen: InventoryScreen,
) -> Option<InventoryCellHit> {
    let origin = screens::panel_origin(screen, [geometry.gui_width, geometry.gui_height]);
    let local = [point[0] - origin[0], point[1] - origin[1]];
    if let InventoryScreen::Creative = screen
        && let Some(tab) = screens::tab_at(local)
    {
        return Some(if tab == screens::SEARCH_TAB {
            InventoryCellHit::CreativeSearch
        } else {
            InventoryCellHit::CreativeTab(tab)
        });
    }
    if let InventoryScreen::Window(kind, _) = screen
        && let Some((widget, _, _)) =
            screens::widget_rects(kind)
                .into_iter()
                .find(|(_, pos, size)| {
                    local[0] >= pos[0]
                        && local[0] < pos[0] + size[0]
                        && local[1] >= pos[1]
                        && local[1] < pos[1] + size[1]
                })
    {
        return Some(InventoryCellHit::Widget(widget));
    }
    screens::slot_at(&screens::screen_slots(screen), local).map(|slot| slot.hit)
}

#[cfg(test)]
mod tests {
    use super::screens::SLOT_SIZE;
    use super::*;

    const PANEL_SIZE: [f32; 2] = [176.0, 166.0];
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
    fn generic_storage_hit_testing_is_exact_for_server_menu_rows() {
        let geometry = geometry([1280, 720], 1.0, SafeArea::ZERO);
        let Some(protocol::OpenCells::Generic(lengths)) = WindowKind::Storage.open_cells() else {
            panic!("storage cell contract");
        };
        for &count in lengths {
            let rows = count / 9;
            let panel_height = 114.0 + rows as f32 * SLOT_SIZE;
            let origin = [
                ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
                ((geometry.gui_height - panel_height) * 0.5).floor(),
            ];
            assert_eq!(
                cell_hit(
                    [origin[0] + 9.0, origin[1] + 19.0],
                    geometry,
                    InventoryScreen::Storage(count)
                ),
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
                    InventoryScreen::Storage(count),
                ),
                Some(InventoryCellHit::Storage(last as u8))
            );
            assert_eq!(
                cell_hit(
                    [origin[0] + 9.0, origin[1] + 33.0 + rows as f32 * SLOT_SIZE],
                    geometry,
                    InventoryScreen::Storage(count),
                ),
                Some(InventoryCellHit::Player(9))
            );
        }
    }

    #[test]
    fn admitted_server_menu_rows_choose_storage_instead_of_personal_inventory() {
        use protocol::{
            ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
            NetworkItemStack,
        };
        let Some(protocol::OpenCells::Generic(lengths)) = WindowKind::Storage.open_cells() else {
            panic!("storage cell contract");
        };
        for &count in lengths {
            let mut ledger = crate::ui_runtime::inventory_ledger::PlayerInventoryLedger::default();
            ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type: protocol::WINDOW_TYPE_CONTAINER,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                    ..ContainerIdentity::window(7)
                },
                slots: vec![NetworkItemStack::empty(); count].into(),
                storage_item: NetworkItemStack::empty(),
            }));
            assert_eq!(
                InventoryScreen::of(&ledger),
                InventoryScreen::Storage(count)
            );
        }
    }

    /// Grids, output, armor and offhand resolve to their own cells on each
    /// screen; the workbench offers no equipment cells.
    #[test]
    fn crafting_and_equipment_cells_resolve_per_screen() {
        let geometry = geometry([1280, 720], 1.0, SafeArea::ZERO);
        let origin = [
            ((geometry.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
            ((geometry.gui_height - PANEL_SIZE[1]) * 0.5).floor(),
        ];
        let hit = |offset: [f32; 2], screen| {
            cell_hit(
                [origin[0] + offset[0] + 1.0, origin[1] + offset[1] + 1.0],
                geometry,
                screen,
            )
        };
        let personal = InventoryScreen::Personal;
        assert_eq!(
            hit([98.0, 18.0], personal),
            Some(InventoryCellHit::Craft(28))
        );
        assert_eq!(
            hit([116.0, 36.0], personal),
            Some(InventoryCellHit::Craft(31))
        );
        assert_eq!(
            hit([152.0, 28.0], personal),
            Some(InventoryCellHit::CraftOutput)
        );
        assert_eq!(hit([8.0, 62.0], personal), Some(InventoryCellHit::Armor(3)));
        assert_eq!(hit([77.0, 62.0], personal), Some(InventoryCellHit::Offhand));
        let workbench = InventoryScreen::Workbench;
        assert_eq!(
            hit(WORKBENCH_GRID, workbench),
            Some(InventoryCellHit::Craft(32))
        );
        assert_eq!(
            hit(
                [WORKBENCH_GRID[0] + 36.0, WORKBENCH_GRID[1] + 36.0],
                workbench
            ),
            Some(InventoryCellHit::Craft(40))
        );
        assert_eq!(
            hit(WORKBENCH_OUTPUT, workbench),
            Some(InventoryCellHit::CraftOutput)
        );
        assert_eq!(hit([8.0, 8.0], workbench), None);
        assert_eq!(
            hit([8.0, 84.0], workbench),
            Some(InventoryCellHit::Player(9))
        );
    }
}
