//! Container screens through the engine whenever the UI carrier loaded; without
//! it, or after a failed render, the Java-styled screens draw. The personal inventory and workbench
//! draw from the vanilla crafting screens in their classic survival layout; a
//! storage window picks its screen from its container type. Slot data feeds the
//! vanilla collections; item icons reach `inventory_item_renderer` through an
//! index into this frame's icon table, and `#hover_text` feeds the tooltips.

use json_ui::{
    CollectionItem, DataSource, Draw, DrawNode, RectOut, Scalar, TextAlign, ViewState, hit_test,
};
use protocol::NetworkItemStack;
use serde_json::Value;
use std::sync::Arc;
use ui::UiNode;

use super::super::{HudFrame, IconRef, TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::container_data;
use super::container_kinds::{
    Cell, ContainerKind, mount_kind, mount_slots, storage_kind, window_kind,
};
use super::engine;
use crate::ui_runtime::{
    UiRuntime,
    forms::EngineFrame,
    inventory_ledger::InventoryTarget,
    presentation::inventory_pointer::{InventoryCellHit, InventoryScreen},
};

/// First UI inventory slot of the personal 2x2 and the workbench 3x3 grids.
const PERSONAL_CRAFT_SLOT: u8 = 28;
const WORKBENCH_CRAFT_SLOT: u8 = 32;
/// A clip wide enough to never cut the held stack (virtual px).
const UNCLIPPED: f64 = 1.0e5;

/// Which screen the engine drew, for mapping its cells back to ledger targets.
#[derive(Clone, Copy, Debug)]
pub(super) enum ScreenLayout {
    /// The survival inventory (and the creative one); `book` shows the recipe book.
    Personal {
        book: bool,
    },
    Workbench {
        book: bool,
    },
    /// A chest-like storage window or a station.
    Station(&'static ContainerKind),
    /// A book reader or editor, or a lectern's book.
    Book,
}

impl ScreenLayout {
    /// `block_entity` is the open block entity's NBT `id`, which picks the chest variant.
    pub(super) fn of(
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        block_entity: Option<&str>,
    ) -> Option<Self> {
        let book = super::recipe_book::recipe_book_shown(player_runtime, runtime);
        Some(match InventoryScreen::of_runtime(player_runtime, runtime) {
            InventoryScreen::Personal | InventoryScreen::Creative => Self::Personal { book },
            InventoryScreen::Workbench => Self::Workbench { book },
            InventoryScreen::Storage(slots) => Self::Station(storage_kind(slots, block_entity)),
            InventoryScreen::Window(protocol::WindowKind::Horse, _) => Self::Station(mount_kind(
                mount_slots(runtime.screen_state().mount_identifier.as_deref()),
            )),
            InventoryScreen::Window(kind, _) => Self::Station(window_kind(kind)?),
            InventoryScreen::Book => Self::Book,
        })
    }

    pub(super) fn screen(self) -> (&'static str, &'static str) {
        match self {
            Self::Personal { .. } => ("crafting.inventory_screen", "container.crafting"),
            Self::Workbench { .. } => ("crafting.crafting_screen", "container.crafting"),
            Self::Station(kind) => (kind.screen, kind.title_key),
            Self::Book => (super::book_screen::SCREEN, "book.editTitle"),
        }
    }

    fn craft_slot(self) -> u8 {
        match self {
            Self::Workbench { .. } => WORKBENCH_CRAFT_SLOT,
            _ => PERSONAL_CRAFT_SLOT,
        }
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
            ScreenLayout::of(player_runtime, runtime, window_text.block_entity.as_deref())
        else {
            return Ok(false);
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(false);
        };
        let (reference, title_key) = layout.screen();
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
        let mut context = super::menu_screens::retail_context()
            .with_var("container_title", Value::String(title.clone()))
            .with_flag("localize_title", false);
        match layout {
            ScreenLayout::Station(kind) => {
                for flag in kind.flags {
                    context = context.with_flag(flag, true);
                }
                if super::furnace_book::active(player_runtime) {
                    context = super::furnace_book::context(context);
                }
            }
            ScreenLayout::Book => context = super::book_screen::context(context),
            _ => context = super::recipe_book::context(context),
        }
        let mut icons = Vec::new();
        let data = screen_data(
            player_runtime,
            runtime,
            &self.hud_frame,
            layout,
            &title,
            &mut icons,
            (
                &mut self.form_presentation.book_cache,
                &mut self.form_presentation.furnace_cache,
            ),
        );
        let pointer = runtime.inventory_pointer_gui();
        let view = ViewState {
            hovered: previous
                .zip(pointer)
                .and_then(|(frame, point)| {
                    hit_test(&frame.hits, [f64::from(point[0]), f64::from(point[1])])
                })
                .map(|region| region.key.clone()),
            // The anvil's name, the search field or the book page shows focused
            // while it takes typing.
            focused: match (previous, &runtime.screen_state().book) {
                (Some(frame), Some(book)) => super::book_screen::focused(&frame.hits, book),
                _ => previous
                    .filter(|_| runtime.screen_state().text_focused())
                    .and_then(|frame| {
                        frame
                            .hits
                            .iter()
                            .find(|region| region.kind == json_ui::HitKind::EditBox)
                    })
                    .map(|region| region.key.clone()),
            },
            scroll: runtime.screen_state().container_scroll.clone(),
            ..ViewState::default()
        };
        let overlay = held_stack(
            runtime.inventory_ledger(player_runtime).cursor_stack(),
            self.hud_frame.cursor_icon,
            pointer,
            &mut icons,
        );
        let id_aux = container_data::id_aux_icons(player_runtime, runtime, &self.hud_frame, |id| {
            self.item_icon(id, 0)
        });
        // The hovered item's full tooltip (name, enchantments, lore) replaces its
        // bound name, so hovering changes no data.
        let tooltip = runtime
            .screen_state()
            .hover
            .filter(|hit| hit.is_item_cell())
            .and_then(|_| tooltip_text(&self.hud_frame.window_text.tooltip));
        let preview_view = std::cell::Cell::new(None);
        let art = engine::ScreenArt {
            icons: &icons,
            id_aux: &id_aux,
            view: Some(&view),
            tooltip: tooltip.as_deref(),
            preview: self.hud_frame.player_preview,
            preview_view: Some(&preview_view),
            pointer,
            now: self.menu_seconds,
            clocks: Some(&self.scene_clock),
            ..engine::ScreenArt::default()
        };
        let translate = |key: &str| runtime.translation(key);
        let rollback = (nodes.len(), *next);
        let inputs = engine::EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = engine::EngineOutput {
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &overlay,
        };
        let cache = &mut self.form_presentation.container_cache;
        let catalog = renderer.catalog();
        let drawn = renderer.draw(art, inputs, out, |env, root| {
            ScreenCache::render(
                cache,
                catalog,
                reference,
                &context,
                &data,
                &view,
                root,
                env,
                (metrics.scale.get(), runtime.text_generation()),
            )
        });
        if let Some(view) = preview_view.get() {
            self.player_preview_view = view;
        }
        match drawn {
            Ok(Some(frame)) => {
                self.form_presentation.container = Some((frame, layout));
                Ok(true)
            }
            // A screen the engine cannot draw hands containers back to the Java path.
            Ok(None) | Err(_) => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                self.hud_frame.engine_containers = false;
                Ok(false)
            }
        }
    }

    /// Layouts the current container screen has run, for cache tests.
    #[cfg(test)]
    pub fn engine_container_layouts(&self) -> usize {
        self.form_presentation
            .container_cache
            .as_ref()
            .map_or(0, |cache| cache.layouts)
    }

    /// The container frame the engine drew last build, if any.
    pub fn engine_container_frame(&self) -> Option<&EngineFrame> {
        self.form_presentation
            .container
            .as_ref()
            .map(|(frame, _)| frame)
    }

    /// The ledger cell under a container-screen point (virtual px equal GUI px).
    pub fn engine_container_hit(&self, gui: [f32; 2]) -> Option<InventoryCellHit> {
        let (frame, layout) = self.form_presentation.container.as_ref()?;
        let region = hit_test(&frame.hits, [f64::from(gui[0]), f64::from(gui[1])])?;
        let widget = || match *layout {
            ScreenLayout::Station(kind) => {
                super::furnace_book::hit(kind.screen, region).or_else(|| {
                    container_data::widget_hit(kind.screen, region).map(InventoryCellHit::Widget)
                })
            }
            ScreenLayout::Personal { book } | ScreenLayout::Workbench { book } => {
                super::recipe_book::book_hit(region, book)
            }
            ScreenLayout::Book => super::book_screen::book_hit(region),
        };
        if matches!(layout, ScreenLayout::Book) {
            return widget();
        }
        let (Some(index), Some(collection)) =
            (region.collection_index, region.collection.as_deref())
        else {
            return widget();
        };
        let small = u8::try_from(index).ok();
        Some(match collection {
            "inventory_items" => InventoryCellHit::Player(small?.checked_add(9)?),
            "hotbar_items" => InventoryCellHit::Player(small?),
            "armor_items" => InventoryCellHit::Armor(small?),
            "offhand_items" => InventoryCellHit::Offhand,
            "crafting_input_items" => {
                InventoryCellHit::Craft(layout.craft_slot().checked_add(small?)?)
            }
            "crafting_output_items" => InventoryCellHit::CraftOutput,
            collection => match layout {
                ScreenLayout::Station(kind) => match kind.cell(collection, index) {
                    Some(cell) => cell.hit(),
                    None => return widget(),
                },
                _ => return widget(),
            },
        })
    }
}

/// The last container screen's resolved tree, its binding and its layout, each
/// kept while its inputs stay the same: a hover, press or focus change only
/// filters the laid-out nodes, and a screen sitting open reuses it all.
pub(super) struct ScreenCache {
    catalog: Arc<json_ui::Catalog>,
    reference: &'static str,
    context: json_ui::Context,
    resolved: json_ui::ResolvedControl,
    /// The data `tree` or the layout's `FormRender::bound` was bound against.
    data: Option<DataSource>,
    /// The bound tree while no layout holds it, so a relayout never clones it.
    tree: Option<json_ui::ResolvedControl>,
    /// The tree's measurements at the laid root size, reused while scrolling.
    measures: json_ui::MeasureCache,
    laid: Option<(ViewState, [f64; 2], Arc<json_ui::FormRender>)>,
    /// The open screen's live bindings across data refreshes.
    binding: json_ui::BindState,
    /// Font scale and language tables used by the retained measurements.
    text: (f32, [usize; 3]),
    /// Layouts run for this screen, for cache tests.
    layouts: usize,
}

impl ScreenCache {
    #[allow(clippy::too_many_arguments)]
    fn render(
        cache: &mut Option<Self>,
        catalog: &Arc<json_ui::Catalog>,
        reference: &'static str,
        context: &json_ui::Context,
        data: &DataSource,
        view: &ViewState,
        root: [f64; 2],
        env: &json_ui::LayoutEnv,
        text: (f32, [usize; 3]),
    ) -> Option<Arc<json_ui::FormRender>> {
        let same_screen = |cached: &Self| {
            Arc::ptr_eq(&cached.catalog, catalog)
                && cached.reference == reference
                && cached.context == *context
        };
        if !cache.as_ref().is_some_and(same_screen) {
            *cache = Some(Self {
                catalog: Arc::clone(catalog),
                reference,
                context: context.clone(),
                resolved: json_ui::resolve_screen(reference, catalog, context)?,
                data: None,
                tree: None,
                measures: json_ui::MeasureCache::default(),
                laid: None,
                binding: json_ui::BindState::new(),
                text,
                layouts: 0,
            });
        }
        let cached = cache.as_mut()?;
        let text_changed = cached.text != text;
        if text_changed {
            cached.text = text;
            cached.measures = json_ui::MeasureCache::default();
        }
        if cached.data.as_ref() != Some(data) {
            cached.tree = Some(json_ui::bind_screen(
                &cached.resolved,
                catalog,
                context,
                data,
                &mut cached.binding,
            ));
            cached.data = Some(data.clone());
            cached.measures = json_ui::MeasureCache::default();
            cached.laid = None;
        }
        // Hover, press and focus only filter the gated nodes; scroll lays out again.
        let fresh = |(laid_view, laid_root, _): &(ViewState, [f64; 2], _)| {
            !text_changed && laid_view.scroll == view.scroll && *laid_root == root
        };
        if !cached.laid.as_ref().is_some_and(fresh) {
            if cached
                .laid
                .as_ref()
                .is_some_and(|(_, laid_root, _)| *laid_root != root)
            {
                cached.measures = json_ui::MeasureCache::default();
            }
            let tree = match cached.tree.take() {
                Some(tree) => tree,
                None => {
                    let (_, _, render) = cached.laid.take()?;
                    Arc::try_unwrap(render)
                        .map_or_else(|shared| shared.bound.clone(), |render| render.bound)
                }
            };
            let render = json_ui::render_bound_gated(tree, root, env, view, &mut cached.measures);
            // Scroll views publish their end state; views reading it rebind next frame.
            if cached.binding.publish_scrolls(&render.report) && cached.binding.observes_scroll() {
                cached.data = None;
            }
            cached.layouts += 1;
            cached.laid = Some((view.clone(), root, Arc::new(render)));
        }
        cached
            .laid
            .as_ref()
            .map(|(_, _, render)| Arc::clone(render))
    }
}

/// Whether the engine has a vanilla screen for the open inventory or window.
pub fn engine_screen_for(player_runtime: &player_state::PlayerState, runtime: &UiRuntime) -> bool {
    ScreenLayout::of(player_runtime, runtime, None).is_some()
}

/// The vanilla screen the open inventory or container draws.
pub fn container_screen_reference(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Option<&'static str> {
    ScreenLayout::of(player_runtime, runtime, None).map(|layout| layout.screen().0)
}

/// Whether a point lies on the engine-drawn container's `root_panel`.
pub fn engine_panel_contains(frame: &EngineFrame, gui: [f32; 2]) -> bool {
    let (x, y) = (f64::from(gui[0]), f64::from(gui[1]));
    frame
        .panel
        .is_some_and(|[px, py, pw, ph]| x >= px && x < px + pw && y >= py && y < py + ph)
}

/// Builds one screen's collections, pushing each drawn icon into `icons`.
struct Cells<'a> {
    frame: &'a HudFrame,
    icons: &'a mut Vec<IconRef>,
}

impl Cells<'_> {
    fn cell(
        &mut self,
        stack: Option<&NetworkItemStack>,
        icon: Option<IconRef>,
        durability: Option<f32>,
    ) -> CollectionItem {
        // A retained cell must answer even when empty, otherwise the previous
        // frame's index can name a different icon in the new compact table.
        let mut item =
            CollectionItem::default().with("#item_renderer_data", Scalar::Json(Value::Null));
        if let (Some(_), Some(icon)) = (stack, icon) {
            self.icons.push(icon);
            item = item.with(
                "#item_renderer_data",
                Scalar::Num((self.icons.len() - 1) as f64),
            );
        }
        let count = stack.map_or(0, |stack| stack.count);
        let name = stack
            .and_then(|stack| {
                self.frame
                    .item_names
                    .get(&(stack.network_id, stack.metadata))
            })
            .map_or_else(String::new, |name| name.to_string());
        item.with(
            "#inventory_stack_count",
            Scalar::Text(if count > 1 {
                count.to_string()
            } else {
                String::new()
            }),
        )
        .with("#hover_text", Scalar::Text(name))
        .with("#is_selected_slot", Scalar::Bool(false))
        // The classic cell art; the controller always answers the background.
        .with("#container_item_background", Scalar::Int(0))
        .with("#container_item_modifier", Scalar::Int(0))
        .with(
            "#item_durability_visible",
            Scalar::Bool(durability.is_some()),
        )
        .with("#item_durability_total_amount", Scalar::Num(1000.0))
        .with(
            "#item_durability_current_amount",
            Scalar::Num(f64::from(durability.unwrap_or(1.0)) * 1000.0),
        )
    }
}

/// The player's main inventory (slots 9 to 35) and hotbar as vanilla's `inventory_items` and
/// `hotbar_items` rows, pushing each drawn icon into `icons`.
pub(super) fn player_rows(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    icons: &mut Vec<IconRef>,
) -> [Vec<CollectionItem>; 2] {
    let ledger = runtime.inventory_ledger(player_runtime);
    let mut cells = Cells { frame, icons };
    let player_icon = |index: usize| frame.inventory_icons.0.get(index).copied().flatten();
    let inventory = (9..36)
        .map(|index| {
            cells.cell(
                ledger.displayed_stack(index as u8),
                player_icon(index),
                frame.durability.player[index],
            )
        })
        .collect();
    let hotbar = (0..9)
        .map(|index| {
            cells.cell(
                ledger.displayed_stack(index as u8),
                player_icon(index),
                frame.hotbar_durability[index],
            )
        })
        .collect();
    [inventory, hotbar]
}

fn screen_data(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    layout: ScreenLayout,
    title: &str,
    icons: &mut Vec<IconRef>,
    caches: (
        &mut Option<super::recipe_book::BookCache>,
        &mut Option<super::furnace_book::FurnaceBookCache>,
    ),
) -> DataSource {
    let (book_cache, furnace_cache) = caches;
    let ledger = runtime.inventory_ledger(player_runtime);
    let mut data = DataSource::new();
    // Bindings the controller does not answer read as false, as in vanilla.
    data.set_strict(true);
    let [inventory, hotbar] = player_rows(player_runtime, runtime, frame, icons);
    data.set_collection("inventory_items", inventory);
    data.set_collection("hotbar_items", hotbar);
    let mut cells = Cells { frame, icons };
    survival_globals(&mut data, title);
    match layout {
        ScreenLayout::Personal { book } | ScreenLayout::Workbench { book } => {
            super::recipe_book::book_data(
                player_runtime,
                &mut data,
                runtime,
                frame,
                cells.icons,
                book,
                book_cache,
            );
            let width = if matches!(layout, ScreenLayout::Workbench { .. }) {
                3
            } else {
                2
            };
            let first = layout.craft_slot();
            let grid = (0..width * width)
                .map(|index| {
                    let slot = first + index as u8;
                    cells.cell(
                        ledger.target_stack(InventoryTarget::Craft(slot)),
                        frame.crafting.icons.get(index).copied().flatten(),
                        None,
                    )
                })
                .collect();
            data.set_collection("crafting_input_items", grid);
            let output = match &frame.crafting.output {
                Some((icon, stack)) => cells.cell(Some(stack), *icon, None),
                None => cells.cell(None, None, None),
            };
            data.set_collection("crafting_output_items", vec![output]);
            let armor = (0..4u8)
                .map(|slot| {
                    let stack = ledger.target_stack(InventoryTarget::Armor(slot));
                    cells
                        .cell(stack, frame.armor_icons[usize::from(slot)], None)
                        .with("#empty_armor_image_visible", Scalar::Bool(stack.is_none()))
                })
                .collect();
            data.set_collection("armor_items", armor);
            let offhand = ledger.target_stack(InventoryTarget::Offhand);
            let offhand = cells
                .cell(offhand, frame.offhand_icon, frame.offhand_durability)
                .with(
                    "#empty_offhand_image_visible",
                    Scalar::Bool(offhand.is_none()),
                );
            data.set_collection("offhand_items", vec![offhand]);
        }
        ScreenLayout::Station(kind) => {
            if super::furnace_book::active(player_runtime) {
                super::furnace_book::data(
                    player_runtime,
                    runtime,
                    frame,
                    &mut data,
                    cells.icons,
                    furnace_cache,
                );
            }
            for (collection, addressed) in kind.collections {
                let shown = container_data::collection_len(
                    player_runtime,
                    runtime,
                    collection,
                    addressed.len(),
                );
                let items = addressed[..shown]
                    .iter()
                    .map(|cell| {
                        let (stack, icon, durability) =
                            station_cell(player_runtime, runtime, frame, *cell);
                        let item = cells.cell(stack, icon, durability);
                        container_data::decorate(
                            player_runtime,
                            runtime,
                            collection,
                            stack.is_none(),
                            item,
                        )
                    })
                    .collect();
                data.set_collection(*collection, items);
            }
            if let Some(window) = ledger.window_kind() {
                container_data::station_globals(player_runtime, &mut data, runtime, window);
                container_data::station_controls(player_runtime, &mut data, runtime, frame, window);
            }
        }
        ScreenLayout::Book => {
            if let Some(book) = &runtime.screen_state().book {
                super::book_screen::book_data(&mut data, book);
            }
        }
    }
    data
}

/// Tooltip lines as one `#hover_text`, each coloured by its format code.
pub(super) fn tooltip_text(
    lines: &[crate::ui_runtime::presentation::hud_layout::TooltipLine],
) -> Option<String> {
    // Decode the shared UI formatting palette once; do not duplicate its RGB
    // constants or the parser's extended material-color mapping here.
    static PALETTE: std::sync::OnceLock<Vec<(char, [u8; 3])>> = std::sync::OnceLock::new();
    let palette = PALETTE.get_or_init(|| {
        ('0'..='9')
            .chain('a'..='v')
            .filter_map(|code| {
                let sample = format!("§{code}x");
                let spans = ui::parse_bedrock_text(&sample, sample.len()).ok()?;
                let style = spans.first()?.style;
                (!style.bold && !style.italic && !style.obfuscated && code != 'r')
                    .then(|| (code, style.color.rgb().unwrap_or([255; 3])))
            })
            .collect()
    });
    (!lines.is_empty()).then(|| {
        lines
            .iter()
            .map(|line| {
                let rgb = [line.color[0], line.color[1], line.color[2]];
                let code = palette.iter().find(|(_, color)| *color == rgb);
                // Each TooltipLine was independently styled. Reset before its
                // prefix so a custom name's bold/italic/color cannot leak.
                match code {
                    Some((code, _)) => format!("§r§{code}{}", line.text),
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
mod empty_cells_tests;

/// The stack, icon, and durability a station cell shows.
fn station_cell<'a>(
    player_runtime: &'a player_state::PlayerState,
    runtime: &'a UiRuntime,
    frame: &HudFrame,
    cell: Cell,
) -> (Option<&'a NetworkItemStack>, Option<IconRef>, Option<f32>) {
    let ledger = runtime.inventory_ledger(player_runtime);
    match cell {
        Cell::Storage(slot) => {
            let index = usize::from(slot);
            (
                ledger.furnace_visual_stack(slot),
                frame.storage_icons.0.get(index).copied().flatten(),
                frame.durability.storage.get(index).copied().flatten(),
            )
        }
        Cell::Ui(slot) => {
            let index = usize::from(slot);
            (
                ledger.target_stack(InventoryTarget::Craft(slot)),
                frame.window_icons.ui.get(index).copied().flatten(),
                frame.durability.ui.get(index).copied().flatten(),
            )
        }
        Cell::Output => {
            let index = usize::from(protocol::CREATED_OUTPUT_SLOT);
            (
                ledger.created_output_stack(),
                frame.window_icons.ui.get(index).copied().flatten(),
                frame.durability.ui.get(index).copied().flatten(),
            )
        }
    }
}

/// The classic survival layout on desktop: no recipe book, no creative tabs.
fn survival_globals(data: &mut DataSource, title: &str) {
    for (name, value) in [
        ("#is_survival_layout", true),
        ("#is_recipe_book_layout", false),
        ("#is_creative_mode", false),
        ("#is_creative_layout", false),
        ("#is_creative_layout_button_visible", false),
        ("#is_left_tab_inventory", true),
        ("#needs_crafting_table", false),
        ("#show_persistent_bundle_hover_text", true),
        ("#gamepad_helper_visible", false),
        ("#filtering_enabled", false),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    data.set_global("#crafting_label_text", Scalar::Text(title.to_owned()));
}

/// The held stack drawn under the pointer, above the screen.
fn held_stack(
    stack: Option<&NetworkItemStack>,
    icon: Option<IconRef>,
    pointer: Option<[f32; 2]>,
    icons: &mut Vec<IconRef>,
) -> Vec<DrawNode> {
    let (Some(stack), Some(icon), Some(point)) = (stack, icon, pointer) else {
        return Vec::new();
    };
    icons.push(icon);
    let clip = RectOut {
        x: -UNCLIPPED,
        y: -UNCLIPPED,
        w: UNCLIPPED * 2.0,
        h: UNCLIPPED * 2.0,
    };
    let (x, y) = (f64::from(point[0]) - 8.0, f64::from(point[1]) - 8.0);
    let node = |dest: RectOut, draw: Draw| DrawNode {
        name: "held_item".to_owned(),
        key: String::new(),
        dest,
        clip,
        layer: i32::MAX,
        alpha: 1.0,
        anim: None,
        draw,
        gates: Vec::new(),
    };
    let mut nodes = vec![node(
        RectOut {
            x,
            y,
            w: 16.0,
            h: 16.0,
        },
        Draw::Custom {
            renderer: "inventory_item_renderer".to_owned(),
            data: [(
                "#item_renderer_data".to_owned(),
                Value::from((icons.len() - 1) as u64),
            )]
            .into_iter()
            .collect(),
        },
    )];
    if stack.count > 1 {
        nodes.push(node(
            RectOut {
                x,
                y: y + 8.0,
                w: 17.0,
                h: 9.0,
            },
            Draw::Text {
                text: stack.count.to_string(),
                color: [255; 4],
                shadow: true,
                align: TextAlign::Right,
                scale: 1.0,
                localize: false,
                options: Default::default(),
            },
        ));
    }
    nodes
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_container_layout_remeasures_after_language_or_scale_changes() {
        struct Text(f64);
        impl json_ui::TextMeasure for Text {
            fn extent(&self, _: &str) -> [f64; 2] {
                [self.0, 10.0]
            }
        }
        struct NoTexture;
        impl json_ui::TextureSource for NoTexture {
            /// The measurement fixture has no image sources.
            fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
                None
            }
        }
        let mut catalog = json_ui::Catalog::default();
        catalog.apply_pack([("ui/_ui_defs.json", br#"{"ui_defs":["ui/chest.json"]}"#.as_slice()), ("ui/chest.json", br#"{"namespace":"chest","small_chest_screen":{"type":"screen","controls":[{"label":{"type":"label","text":"key","size":["default",10]}}]}}"#.as_slice())]);
        let catalog = Arc::new(catalog);
        let mut cache = None;
        let (context, data, view) = (
            json_ui::Context::desktop(),
            DataSource::new(),
            ViewState::default(),
        );
        for (generation, width) in [(0, 10.0), (1, 30.0)] {
            let text = Text(width);
            let env = json_ui::LayoutEnv {
                text: &text,
                textures: &NoTexture,
            };
            let rendered = ScreenCache::render(
                &mut cache,
                &catalog,
                "chest.small_chest_screen",
                &context,
                &data,
                &view,
                [100.0; 2],
                &env,
                (1.0, [generation, 0, 0]),
            )
            .unwrap();
            let label = rendered
                .nodes
                .iter()
                .find(|node| node.name == "label")
                .unwrap();
            assert_eq!(label.dest.w, width);
        }
    }
}
