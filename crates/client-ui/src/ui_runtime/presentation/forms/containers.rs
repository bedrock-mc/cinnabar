//! Projects the client's inventory authority and frame into the container screens: which screen
//! the open window draws, its title and tooltip, and the cells its hits address.

use std::{any::Any, sync::Arc};

use protocol::{ScreenRecipe, WindowKind};
use ui::{IconRef, UiNode};
use ui_engine::engine::screen::{CustomRenderer, PaintFrame};
use ui_engine::frame::EngineFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::container_actions::InventoryCellHit;
use ui_screens::container_data::id_aux_icons;
use ui_screens::container_kinds::{mount_kind, storage_kind, window_kind};
use ui_screens::container_source::{
    BookView, ContainerFrame, ContainerSource, NameSources, ScreenView,
};
use ui_screens::containers::{ContainerInput, ScreenLayout};
use ui_screens::furnace_book::Entries;
use ui_screens::recipe_book::BookEntry;

use super::super::{HudFrame, UiPresentationError, UiPresentationRuntime};
use super::preview_renderer::PreviewRenderer;
use crate::ui_runtime::{
    UiRuntime, inventory_actions::recipe_book_entries,
    presentation::inventory_pointer::InventoryScreen,
};
use assets::entity::mount::MountEquipment;

/// The client's authority as the container screens read it.
pub(crate) struct ContainerAuthority<'a> {
    player: &'a player_state::PlayerState,
    runtime: &'a UiRuntime,
    screen: ScreenView<'a>,
}

impl<'a> ContainerAuthority<'a> {
    pub(crate) fn new(player: &'a player_state::PlayerState, runtime: &'a UiRuntime) -> Self {
        let state = runtime.screen_state();
        let screen = ScreenView {
            mount_equipment: state.mount_equipment,
            anvil_name: &state.anvil_name,
            recipe_choice: state.recipe_choice,
            beacon: state.beacon,
            beacon_level: state.beacon_level,
            loom_pattern: state.loom_pattern.as_deref(),
            crafter_disabled: std::array::from_fn(|slot| state.crafter.is_disabled(slot as u8)),
            crafter_powered: state.crafter.powered,
            furnace_book_open: state.furnace_book_open,
            furnace_tab: state.furnace_tab,
            creative_tab: state.creative_tab,
            creative_wide: state.creative_wide,
            creative_expanded: &state.creative_expanded,
            search: &state.search,
            container_scroll: &state.container_scroll,
            text_focused: state.text_focused(),
            book: state.book.as_ref().map(|book| BookView {
                pages: &book.pages,
                page: book.page,
                spread: book.spread(),
                editable: book.editable,
                signing: book.signing,
                title: &book.title,
                author: &book.author,
                editing: book.editing,
            }),
        };
        Self {
            player,
            runtime,
            screen,
        }
    }
}

impl ContainerSource for ContainerAuthority<'_> {
    fn player(&self) -> &player_state::PlayerState {
        self.player
    }

    fn screen(&self) -> &ScreenView<'_> {
        &self.screen
    }

    fn translation(&self, key: &str) -> Option<Arc<str>> {
        self.runtime.translation(key)
    }

    fn experience_level(&self) -> u32 {
        self.runtime.hud().experience().map_or(0, |xp| xp.level)
    }

    fn enchantment_name(&self, id: i16, level: u8) -> String {
        super::super::inventory_tooltip::enchantment_name(self.runtime, id, level)
    }

    fn recipe_filtering(&self) -> bool {
        self.runtime.recipe_filtering(self.player)
    }

    fn item_name_generation(&self) -> u64 {
        self.runtime.item_name_generation()
    }

    fn book_entries(&self) -> Vec<BookEntry<'_>> {
        recipe_book_entries(self.player, self.runtime)
    }

    fn furnace_entries(&self) -> Entries<'_> {
        super::furnace_book::entries(self.player, self.runtime)
    }

    fn furnace_projection(&self) -> Arc<[usize]> {
        super::furnace_book::projection(self.player, self.runtime)
    }

    fn furnace_category(&self, recipe: &ScreenRecipe) -> u8 {
        super::furnace_book::category(self.player, self.runtime, recipe)
    }

    fn name_sources(&self) -> NameSources {
        NameSources {
            components: self
                .runtime
                .session_items
                .clone()
                .map(|items| items as Arc<dyn Any + Send + Sync>),
            lang: self.runtime.lang_catalog.clone(),
        }
    }
}

/// The frame's icons, durabilities and names as the container screens borrow them.
pub(crate) fn container_frame(frame: &HudFrame) -> ContainerFrame<'_> {
    ContainerFrame {
        item_names: &frame.item_names,
        inventory_icons: &frame.inventory_icons.0,
        player_durability: &frame.durability.player,
        hotbar_durability: &frame.hotbar_durability,
        storage_icons: &frame.storage_icons.0,
        storage_durability: &frame.durability.storage,
        ui_icons: &frame.window_icons.ui,
        ui_durability: &frame.durability.ui,
        crafting_icons: &frame.crafting.icons,
        crafting_output: frame.crafting.output.as_ref(),
        armor_icons: &frame.armor_icons,
        offhand_icon: frame.offhand_icon,
        offhand_durability: frame.offhand_durability,
        cursor_icon: frame.cursor_icon,
        recipe_icons: &frame.window_icons.recipe,
        recipe_output: frame.window_icons.recipe_output.as_ref(),
        book_entries: &frame.window_icons.book_entries,
        furnace_entries: &frame.window_icons.furnace_entries,
    }
}

/// The engine screen the open inventory or window draws; `block_entity` is the open block
/// entity's NBT `id`, which picks the chest variant.
pub(super) fn screen_layout(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    block_entity: Option<&str>,
) -> Option<ScreenLayout> {
    let book = super::recipe_book::recipe_book_shown(player_runtime, runtime);
    Some(match InventoryScreen::of_runtime(player_runtime, runtime) {
        InventoryScreen::Personal | InventoryScreen::Creative => ScreenLayout::Personal { book },
        InventoryScreen::Workbench => ScreenLayout::Workbench { book },
        InventoryScreen::Storage(slots) => ScreenLayout::Station(storage_kind(slots, block_entity)),
        InventoryScreen::Window(WindowKind::Horse, _) => {
            ScreenLayout::Station(mount_kind(runtime.screen_state().mount_equipment))
        }
        InventoryScreen::Window(kind, _) => ScreenLayout::Station(window_kind(kind)?),
        InventoryScreen::Book => ScreenLayout::Book,
    })
}

/// Whether the engine has a vanilla screen for the open inventory or window.
pub fn engine_screen_for(player_runtime: &player_state::PlayerState, runtime: &UiRuntime) -> bool {
    screen_layout(player_runtime, runtime, None).is_some()
}

/// The vanilla screen the open inventory or container draws.
pub fn container_screen_reference(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Option<&'static str> {
    screen_layout(player_runtime, runtime, None).map(|layout| layout.screen().0)
}

/// The player's main inventory and hotbar rows, as the container screens bind them.
pub(super) fn player_rows(
    player_runtime: &player_state::PlayerState,
    frame: &HudFrame,
    icons: &mut Vec<IconRef>,
) -> [Vec<json_ui::CollectionItem>; 2] {
    ui_screens::containers::player_rows(player_runtime, &container_frame(frame), icons)
}

/// Reads the open station's block for its screen: the beacon's pyramid level,
/// the crafter's disabled slots and its `triggered_bit`.
pub fn observe_station_block(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    stream: Option<&chunk_pipeline::WorldStream>,
    station_triggered: impl FnOnce([i32; 3]) -> Option<bool>,
    now_millis: u64,
) {
    let ledger = runtime.inventory_ledger(player_runtime);
    let (Some(kind), Some(position), Some(stream)) =
        (ledger.window_kind(), ledger.window_position(), stream)
    else {
        return;
    };
    if kind == WindowKind::Horse {
        let identifier = ledger
            .window_actor()
            .and_then(|unique| stream.authority().actor_by_unique_id(unique))
            .and_then(|actor| match &actor.kind {
                protocol::ActorKind::Entity { identifier } => Some(identifier.clone()),
                protocol::ActorKind::Player { .. } => None,
            });
        let equipment = ledger
            .window_actor()
            .map_or(MountEquipment::Unresolved, |unique| {
                stream.authority().actor_mount_equipment(unique)
            });
        runtime.screen_state_mut().mount_identifier = identifier;
        runtime.screen_state_mut().mount_equipment = equipment;
        return;
    }
    let nbt = stream.block_entity_compound(position);
    let integer = |key: &str| nbt.as_ref().and_then(|nbt| nbt.integer(key));
    match kind {
        WindowKind::Beacon => {
            runtime.screen_state_mut().beacon_level =
                integer("Levels").and_then(|levels| u8::try_from(levels).ok());
        }
        WindowKind::Crafter => {
            let disabled = integer("disabled_slots").map_or(0, |mask| mask as u16);
            let powered = station_triggered(position);
            runtime
                .screen_state_mut()
                .crafter
                .observe(disabled, powered, now_millis);
        }
        _ => {}
    }
}

impl UiPresentationRuntime {
    /// Draw the open inventory or container through the engine. `Ok(false)`
    /// leaves the screen to the Java-styled path.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_engine_container(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        previous: Option<&EngineFrame>,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<bool, UiPresentationError> {
        if !self.hud_frame.engine_containers || !runtime.inventory_open() {
            return Ok(false);
        }
        let window_text = &self.hud_frame.window_text;
        let Some(layout) =
            screen_layout(player_runtime, runtime, window_text.block_entity.as_deref())
        else {
            return Ok(false);
        };
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        let (_, title_key) = layout.screen();
        // A mount's screen is titled with its own entity name.
        let mount_key = runtime
            .screen_state()
            .mount_identifier
            .as_deref()
            .and_then(|id| id.strip_prefix("minecraft:"))
            .map(|name| format!("entity.{name}.name"));
        let title_key = mount_key.as_deref().unwrap_or(title_key);
        // A custom name shows as stated, else the block's own title.
        let title = window_text.custom_title.clone().unwrap_or_else(|| {
            runtime
                .translation(title_key)
                .map_or_else(|| title_key.to_owned(), |title| title.to_string())
        });
        let source = ContainerAuthority::new(player_runtime, runtime);
        let items = container_frame(&self.hud_frame);
        let id_aux = id_aux_icons(&source, &items, |id| self.item_icon(id, 0));
        // The hovered item's full tooltip (name, enchantments, lore) replaces its
        // bound name, so hovering changes no data.
        let tooltip = runtime
            .screen_state()
            .hover
            .filter(|hit| hit.is_item_cell())
            .and_then(|_| tooltip_text(&window_text.tooltip));
        let preview_view = std::cell::Cell::new(None);
        let mut preview = PreviewRenderer {
            preview: self.hud_frame.player_preview,
            view: Some(&preview_view),
            ..PreviewRenderer::default()
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut preview];
        let translate = |key: &str| runtime.translation(key);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut renderers,
        };
        let drawn = self.screens.containers.paint(
            renderer,
            &source,
            &items,
            ContainerInput {
                layout,
                title: &title,
                previous,
                pointer: runtime.inventory_pointer_gui(),
                tooltip: tooltip.as_deref(),
                clocks: &self.scene_clock,
                id_aux: &id_aux,
            },
            &mut paint_frame,
        );
        if let Some(view) = preview_view.get() {
            self.player_preview_view = view;
        }
        match drawn {
            Some(frame) => {
                self.projections.container = Some((frame, layout));
                Ok(true)
            }
            // A screen the engine cannot draw hands containers back to the Java path.
            None => {
                self.hud_frame.engine_containers = false;
                Ok(false)
            }
        }
    }

    /// Layouts the current container screen has run, for cache tests.
    #[cfg(test)]
    pub fn engine_container_layouts(&self) -> usize {
        self.screens.containers.layouts()
    }

    /// The container frame the engine drew last build, if any.
    pub fn engine_container_frame(&self) -> Option<&EngineFrame> {
        self.projections.container.as_ref().map(|(frame, _)| frame)
    }

    /// The ledger cell under a container-screen point (virtual px equal GUI px).
    pub fn engine_container_hit(&self, gui: [f32; 2]) -> Option<InventoryCellHit> {
        let (frame, layout) = self.projections.container.as_ref()?;
        layout.hit(frame, gui)
    }
}

/// Tooltip lines as one `#hover_text`, each coloured by its format code.
pub(super) fn tooltip_text(
    lines: &[crate::ui_runtime::presentation::hud_layout::TooltipLine],
) -> Option<String> {
    (!lines.is_empty()).then(|| {
        lines
            .iter()
            .map(|line| {
                let rgb = [line.color[0], line.color[1], line.color[2]];
                let descriptor = ui::FORMATTING_COLORS
                    .iter()
                    .find(|entry| entry.name.is_some() && entry.fallback_rgb == rgb);
                // Each TooltipLine was independently styled. Reset before its
                // prefix so a custom name's bold/italic/color cannot leak.
                match descriptor {
                    Some(entry) => format!("§r§{}{}", entry.code, line.text),
                    None => format!("§r{}", line.text),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

#[cfg(test)]
mod tooltip_tests;

#[cfg(test)]
mod book_order_tests;
