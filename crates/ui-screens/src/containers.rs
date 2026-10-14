//! Container screens through the engine whenever the UI carrier loaded; without
//! it, or after a failed render, the Java-styled screens draw. The personal inventory and workbench
//! draw from the vanilla crafting screens in their classic survival layout; a
//! storage window picks its screen from its container type. Slot data feeds the
//! vanilla collections; item icons reach `inventory_item_renderer` through an
//! index into this frame's icon table, and `#hover_text` feeds the tooltips.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{
    CollectionItem, DataSource, Draw, DrawNode, RectOut, Scalar, TextAlign, ViewState, hit_test,
    press_region,
};
use protocol::NetworkItemStack;
use serde_json::Value;
use ui::IconRef;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    item_renderer::ItemRenderer,
    screen::{CustomRenderer, PaintFrame},
};
use ui_engine::frame::EngineFrame;

use crate::container_actions::{InventoryCellHit, Widget};
use crate::container_data;
use crate::container_kinds::{Cell, ContainerKind};
use crate::container_source::{ContainerFrame, ContainerSource};
use crate::{book_screen, furnace_book, recipe_book};
use inventory::inventory_ledger::InventoryTarget;

/// First UI inventory slot of the personal 2x2 and the workbench 3x3 grids.
const PERSONAL_CRAFT_SLOT: u8 = 28;
const WORKBENCH_CRAFT_SLOT: u8 = 32;
/// A clip wide enough to never cut the held stack (virtual px).
const UNCLIPPED: f64 = 1.0e5;

/// Which screen the engine drew, for mapping its cells back to ledger targets.
#[derive(Clone, Copy, Debug)]
pub enum ScreenLayout {
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
    /// The vanilla screen this layout draws and its title key.
    pub fn screen(self) -> (&'static str, &'static str) {
        match self {
            Self::Personal { .. } => ("crafting.inventory_screen", "container.crafting"),
            Self::Workbench { .. } => ("crafting.crafting_screen", "container.crafting"),
            Self::Station(kind) => (kind.screen, kind.title_key),
            Self::Book => (book_screen::SCREEN, "book.editTitle"),
        }
    }

    fn craft_slot(self) -> u8 {
        match self {
            Self::Workbench { .. } => WORKBENCH_CRAFT_SLOT,
            _ => PERSONAL_CRAFT_SLOT,
        }
    }

    /// The ledger cell or control under a point of `frame`, which this layout drew (virtual px
    /// equal GUI px).
    pub fn hit(self, frame: &EngineFrame, gui: [f32; 2]) -> Option<InventoryCellHit> {
        let region = hit_test(&frame.hits, [f64::from(gui[0]), f64::from(gui[1])])?;
        if !matches!(self, ScreenLayout::Book) && region.pressed.as_deref() == Some(CLOSE_BUTTON) {
            return Some(InventoryCellHit::Widget(Widget::Close));
        }
        let widget = || match self {
            ScreenLayout::Station(kind) => furnace_book::hit(kind.screen, region).or_else(|| {
                container_data::widget_hit(kind.screen, region).map(InventoryCellHit::Widget)
            }),
            ScreenLayout::Personal { book } | ScreenLayout::Workbench { book } => {
                recipe_book::book_hit(region, book)
            }
            ScreenLayout::Book => book_screen::book_hit(region),
        };
        if matches!(self, ScreenLayout::Book) {
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
                InventoryCellHit::Craft(self.craft_slot().checked_add(small?)?)
            }
            "crafting_output_items" => InventoryCellHit::CraftOutput,
            collection => match self {
                ScreenLayout::Station(kind) => match kind.cell(collection, index) {
                    Some(cell) => cell.hit(),
                    None => return widget(),
                },
                _ => return widget(),
            },
        })
    }
}

/// The container screens' retained layout and recipe rows, carried across builds.
#[derive(Default)]
pub struct ContainerScreen {
    /// The last container screen's layout, other than the personal inventory.
    cache: Option<ScreenCache>,
    /// The personal inventory's layout, kept apart so other containers never evict it and opening
    /// it stays cheap after [`ContainerScreen::prewarm_inventory`].
    inventory: Option<ScreenCache>,
    /// The catalog the inventory was last prewarmed for, so each catalog prewarms once.
    prewarmed: Option<Arc<json_ui::Catalog>>,
    /// Screens resolved from the catalog, for cache tests.
    resolves: usize,
    /// Immutable creative rows reused across hover and scroll frames.
    book: Option<recipe_book::BookCache>,
    furnace: Option<furnace_book::FurnaceBookCache>,
}

/// What one container paint reads beyond the client's authority.
pub struct ContainerInput<'a> {
    pub layout: ScreenLayout,
    /// The window's title; a custom name shows as stated.
    pub title: &'a str,
    /// Last build's frame of this screen, for this build's hover and focus.
    pub previous: Option<&'a EngineFrame>,
    /// The pointer in GUI px.
    pub pointer: Option<[f32; 2]>,
    /// The hovered item's full tooltip (name, enchantments, lore), which replaces its bound name.
    pub tooltip: Option<&'a str>,
    /// The scene's animation clocks.
    pub clocks: &'a BTreeMap<String, f64>,
    /// Icons `#item_id_aux` renderers name, from [`container_data::id_aux_icons`].
    pub id_aux: &'a [(i64, IconRef)],
}

impl ContainerScreen {
    /// Draws the open inventory or container through `engine`; `None` leaves the screen to the
    /// Java-styled path with nothing painted. `frame`'s renderers draw after the item renderer.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        items: &ContainerFrame<'_>,
        input: ContainerInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Option<EngineFrame> {
        self.render(engine, source, items, input, frame, false)
    }

    /// Prepares layout and textures without retaining bindings or painting the closed screen.
    pub fn prepare(
        &mut self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        items: &ContainerFrame<'_>,
        input: ContainerInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) {
        let _ = self.render(engine, source, items, input, frame, true);
    }

    /// Shares container data and layout preparation between prewarming and visible painting.
    fn render(
        &mut self,
        engine: &FormEngine,
        source: &dyn ContainerSource,
        items: &ContainerFrame<'_>,
        input: ContainerInput<'_>,
        frame: &mut PaintFrame<'_>,
        prepare: bool,
    ) -> Option<EngineFrame> {
        let ContainerInput {
            layout,
            title,
            previous,
            pointer,
            tooltip,
            clocks,
            id_aux,
        } = input;
        let (reference, _) = layout.screen();
        let mut context = engine
            .context()
            .clone()
            .with_var("container_title", Value::String(title.to_owned()))
            .with_flag("localize_title", false);
        match layout {
            ScreenLayout::Station(kind) => {
                for flag in kind.flags {
                    context = context.with_flag(flag, true);
                }
                if furnace_book::active(source.player()) {
                    context = furnace_book::context(context);
                }
            }
            ScreenLayout::Book => context = book_screen::context(context),
            _ => context = recipe_book::context(context),
        }
        let mut icons = Vec::new();
        let data = screen_data(
            source,
            items,
            layout,
            title,
            &mut icons,
            (&mut self.book, &mut self.furnace),
        );
        let screen = source.screen();
        let view = ViewState {
            hovered: previous
                .zip(pointer)
                .and_then(|(frame, point)| {
                    hit_test(&frame.hits, [f64::from(point[0]), f64::from(point[1])])
                })
                .map(|region| region.key.clone()),
            // The anvil's name, the search field or the book page shows focused
            // while it takes typing.
            focused: match (previous, &screen.book) {
                (Some(frame), Some(book)) => book_screen::focused(&frame.hits, book),
                _ => previous
                    .filter(|_| screen.text_focused)
                    .and_then(|frame| {
                        frame
                            .hits
                            .iter()
                            .find(|region| region.kind == json_ui::HitKind::EditBox)
                    })
                    .map(|region| region.key.clone()),
            },
            scroll: screen.container_scroll.clone(),
            ..ViewState::default()
        };
        let overlay = held_stack(
            source.player().inventory.ledger().cursor_stack(),
            items.cursor_icon,
            pointer,
            &mut icons,
        );
        let art = ScreenArt {
            view: Some(&view),
            tooltip,
            pointer,
            clocks: Some(clocks),
            overlay: &overlay,
            ..ScreenArt::default()
        };
        let mut item_renderer = ItemRenderer {
            icons: &icons,
            id_aux,
        };
        let rollback = (frame.nodes.len(), *frame.next);
        let text = (frame.metrics.scale.get(), frame.language);
        let caller = std::mem::take(&mut frame.renderers);
        let cache = if matches!(layout, ScreenLayout::Personal { .. }) {
            &mut self.inventory
        } else {
            &mut self.cache
        };
        let catalog = engine.catalog();
        let resolved = |cache: &Option<ScreenCache>| {
            cache.as_ref().map(|cached| Arc::as_ptr(&cached.resolved))
        };
        let before = resolved(cache);
        // Server ui files always come with a new catalog, which rebuilds the screen's binding.
        let server_ui = engine.server_ui;
        let drawn = {
            let mut renderers: Vec<&mut dyn CustomRenderer> = Vec::with_capacity(caller.len() + 1);
            renderers.push(&mut item_renderer);
            for renderer in caller.iter_mut() {
                renderers.push(&mut **renderer);
            }
            let mut paint_frame = frame.with_renderers(&mut renderers);
            let draw = |env: &json_ui::LayoutEnv<'_>, root| {
                ScreenCache::render(
                    cache, catalog, server_ui, reference, &context, &data, &view, root, env, text,
                )
            };
            if prepare {
                let _ = engine.prepare(art, &mut paint_frame, draw);
                if let Some(cached) = cache {
                    // Closed-screen data is incomplete. The first paint binds from fresh state,
                    // even if its data happens to match the prepared layout's inputs.
                    cached.binding = recipe_book::screen_binding(server_ui);
                    cached.data = None;
                }
                Ok(None)
            } else {
                engine.draw(art, &mut paint_frame, draw)
            }
        };
        frame.renderers = caller;
        if resolved(cache) != before {
            self.resolves += 1;
        }
        match drawn {
            Ok(Some(drawn)) => Some(drawn),
            // A screen the engine cannot draw hands containers back to the Java path.
            Ok(None) | Err(_) => {
                frame.nodes.truncate(rollback.0);
                *frame.next = rollback.1;
                None
            }
        }
    }

    /// Releases texture-dependent measurements and layout while retaining the bound screen.
    pub fn invalidate_textures(&mut self) {
        for cache in [&mut self.cache, &mut self.inventory].into_iter().flatten() {
            cache.invalidate_textures();
        }
    }

    /// Layouts the retained container screens have run.
    pub fn layouts(&self) -> usize {
        [&self.cache, &self.inventory]
            .into_iter()
            .flatten()
            .map(|cache| cache.layouts)
            .sum()
    }

    /// Screens the container paints have resolved from the catalog.
    pub fn resolves(&self) -> usize {
        self.resolves
    }

    /// Whether the personal inventory needs preparation for this catalog. An inventory already
    /// painted retains its live bindings and needs no prewarm when it closes.
    pub fn prewarm_inventory(&mut self, engine: &FormEngine) -> bool {
        let catalog = engine.catalog();
        if self
            .prewarmed
            .as_ref()
            .is_some_and(|prewarmed| Arc::ptr_eq(prewarmed, catalog))
        {
            return false;
        }
        self.prewarmed = Some(Arc::clone(catalog));
        !self
            .inventory
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(&cached.catalog, catalog))
    }
}

/// Binds a layout may run to build the grid cells it newly shows.
const GRID_PASSES: usize = 2;

/// `catalog` in `context`, answering from the screen's kept `resolutions`.
fn library<'a>(
    catalog: &'a json_ui::Catalog,
    context: &'a json_ui::Context,
    resolutions: &'a json_ui::ResolveCache,
) -> json_ui::CachedLibrary<'a> {
    json_ui::CachedLibrary {
        library: json_ui::CatalogLibrary { catalog, context },
        cache: resolutions,
    }
}

/// The last container screen's resolved tree, its binding and its layout, each
/// kept while its inputs stay the same: a hover, press or focus change only
/// filters the laid-out nodes, and a screen sitting open reuses it all.
struct ScreenCache {
    catalog: Arc<json_ui::Catalog>,
    reference: &'static str,
    context: json_ui::Context,
    resolved: Arc<json_ui::ResolvedControl>,
    /// The data `tree` or the layout's `FormRender::bound` was bound against.
    data: Option<Arc<DataSource>>,
    /// The bound tree while no layout holds it, so a relayout never clones it.
    tree: Option<json_ui::ResolvedControl>,
    /// The tree's measurements at the laid root size, reused while scrolling.
    measures: json_ui::MeasureCache,
    laid: Option<(ViewState, json_ui::LayoutCacheKey, Arc<json_ui::FormRender>)>,
    /// The open screen's live bindings across data refreshes.
    binding: json_ui::BindState,
    /// Grid and factory templates kept across binds, so their unchanged instances stand.
    resolutions: json_ui::ResolveCache,
    /// Language tables used by the retained measurements.
    language: [usize; 3],
    /// Layouts run for this screen, for cache tests.
    layouts: usize,
}

impl ScreenCache {
    /// The tree the last bind produced, taken back from the layout that holds it. Measurements
    /// address its controls, so a copy taken from a still-shared layout starts them over.
    fn bound_tree(&mut self) -> Option<json_ui::ResolvedControl> {
        if let Some(tree) = self.tree.take() {
            return Some(tree);
        }
        let (_, _, render) = self.laid.take()?;
        Some(Arc::try_unwrap(render).map_or_else(
            |shared| {
                self.measures = json_ui::MeasureCache::default();
                shared.bound.clone()
            },
            |render| render.bound,
        ))
    }

    /// Releases texture-dependent measurements and layout while retaining the live bound tree.
    fn invalidate_textures(&mut self) {
        if let Some((_, _, render)) = self.laid.take()
            && self.tree.is_none()
        {
            self.tree = Some(
                Arc::try_unwrap(render)
                    .map_or_else(|shared| shared.bound.clone(), |render| render.bound),
            );
        }
        self.measures = json_ui::MeasureCache::default();
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        cache: &mut Option<Self>,
        catalog: &Arc<json_ui::Catalog>,
        server_ui: bool,
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
                resolved: Arc::new(json_ui::resolve_screen(reference, catalog, context)?),
                data: None,
                tree: None,
                measures: json_ui::MeasureCache::default(),
                laid: None,
                binding: recipe_book::screen_binding(server_ui),
                resolutions: json_ui::ResolveCache::default(),
                language: text.1,
                layouts: 0,
            });
        }
        let cached = cache.as_mut()?;
        let layout = json_ui::LayoutCacheKey::new(root, text.0, env);
        let text_changed = cached.language != text.1;
        let layout_changed = cached
            .laid
            .as_ref()
            .is_some_and(|(_, previous, _)| *previous != layout);
        if text_changed {
            cached.language = text.1;
        }
        if text_changed || layout_changed {
            cached.measures = json_ui::MeasureCache::default();
        }
        if cached.data.as_deref() != Some(data) {
            let data = Arc::new(data.clone());
            // A refresh rebuilds only the controls it changed and keeps their measurements, so
            // moving one item or switching a tab no longer rebinds and remeasures the screen.
            match cached.bound_tree() {
                Some(mut tree) => {
                    json_ui::rebind_screen(
                        &cached.resolved,
                        &library(&cached.catalog, &cached.context, &cached.resolutions),
                        &data,
                        &mut cached.binding,
                        &mut tree,
                        &mut cached.measures,
                    );
                    cached.tree = Some(tree);
                }
                None => {
                    cached.tree = Some(json_ui::bind_screen_incremental(
                        &cached.resolved,
                        &library(&cached.catalog, &cached.context, &cached.resolutions),
                        &data,
                        &mut cached.binding,
                    ));
                    cached.measures = json_ui::MeasureCache::default();
                }
            }
            cached.data = Some(data);
            cached.laid = None;
        }
        // Hover, press and focus only filter the gated nodes; scroll lays out again.
        let fresh = |(laid_view, laid_layout, _): &(ViewState, json_ui::LayoutCacheKey, _)| {
            !text_changed && laid_view.scroll == view.scroll && *laid_layout == layout
        };
        if !cached.laid.as_ref().is_some_and(fresh) {
            let tree = cached.bound_tree()?;
            let mut render =
                json_ui::render_bound_gated(tree, root, env, view, &mut cached.measures);
            // Long grids build only the cells around those in view: bind what this layout
            // newly shows and lay out again, before anything draws.
            for _ in 0..GRID_PASSES {
                let Some(data) = cached
                    .data
                    .clone()
                    .filter(|_| cached.binding.show_grid_cells(&render.report))
                else {
                    break;
                };
                let mut tree = render.bound;
                json_ui::rebind_screen(
                    &cached.resolved,
                    &library(&cached.catalog, &cached.context, &cached.resolutions),
                    &data,
                    &mut cached.binding,
                    &mut tree,
                    &mut cached.measures,
                );
                render = json_ui::render_bound_gated(tree, root, env, view, &mut cached.measures);
            }
            // Scroll views publish their end state; views reading it rebind next frame.
            if cached.binding.publish_scrolls(&render.report) && cached.binding.observes_scroll() {
                cached.data = None;
            }
            cached.layouts += 1;
            cached.laid = Some((view.clone(), layout, Arc::new(render)));
        }
        cached
            .laid
            .as_ref()
            .map(|(_, _, render)| Arc::clone(render))
    }
}

/// The press targets that drop the held stack, as vanilla's `drop_item_panel`
/// routes them.
const CURSOR_DROPS: [&str; 2] = ["button.cursor_drop_all", "button.cursor_drop_one"];

/// Whether a press at `gui` on the engine-drawn container drops the held stack.
/// The press drops when it reaches no control, since the screen itself maps it
/// to a cursor drop, or reaches a drop panel. Every `common.root_panel` section,
/// such as the inventory, recipe book or creative hotbar, claims presses
/// anywhere on its background, as do its buttons.
pub fn engine_press_drops(frame: &EngineFrame, gui: [f32; 2]) -> bool {
    let point = [f64::from(gui[0]), f64::from(gui[1])];
    press_region(&frame.hits, point).is_none_or(|region| {
        region
            .pressed
            .as_deref()
            .is_some_and(|target| CURSOR_DROPS.contains(&target))
    })
}

/// Builds collections with one icon-table entry per cell, keeping later bindings stable.
struct Cells<'a> {
    names: &'a std::collections::HashMap<(i32, u32), Arc<str>>,
    icons: &'a mut Vec<IconRef>,
}

impl Cells<'_> {
    fn cell(
        &mut self,
        stack: Option<&NetworkItemStack>,
        icon: Option<IconRef>,
        durability: Option<f32>,
    ) -> CollectionItem {
        let index = self.icons.len();
        // Empty cells reserve an entry but never address it: their binding is Null.
        self.icons.push(icon.unwrap_or(IconRef {
            page: 0,
            uv: [0; 4],
            glint: false,
        }));
        let mut item =
            CollectionItem::default().with("#item_renderer_data", Scalar::Json(Value::Null));
        if stack.is_some() && icon.is_some() {
            item = item.with("#item_renderer_data", Scalar::Num(index as f64));
        }
        let count = stack.map_or(0, |stack| stack.count);
        let name = stack
            .and_then(|stack| self.names.get(&(stack.network_id, stack.metadata)))
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
/// `hotbar_items` rows, reserving an icon entry even for empty cells.
pub fn player_rows(
    player_runtime: &player_state::PlayerState,
    frame: &ContainerFrame<'_>,
    icons: &mut Vec<IconRef>,
) -> [Vec<CollectionItem>; 2] {
    let ledger = player_runtime.inventory.ledger();
    let mut cells = Cells {
        names: frame.item_names,
        icons,
    };
    let player_icon = |index: usize| frame.inventory_icons.get(index).copied().flatten();
    let inventory = (9..36)
        .map(|index| {
            cells.cell(
                ledger.displayed_stack(index as u8),
                player_icon(index),
                frame.player_durability[index],
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

/// The bindings `layout` reads; icons the cells name append to `icons`.
pub fn screen_data(
    source: &dyn ContainerSource,
    frame: &ContainerFrame<'_>,
    layout: ScreenLayout,
    title: &str,
    icons: &mut Vec<IconRef>,
    caches: (
        &mut Option<recipe_book::BookCache>,
        &mut Option<furnace_book::FurnaceBookCache>,
    ),
) -> DataSource {
    let (book_cache, furnace_cache) = caches;
    let player_runtime = source.player();
    let ledger = player_runtime.inventory.ledger();
    let mut data = DataSource::new();
    // Bindings the controller does not answer read as false, as in vanilla.
    data.set_strict(true);
    // The survival defaults go first, so the recipe book's layout globals override them.
    survival_globals(&mut data, title);
    // The recipe book's icons go first: its rows name icons by table index, so behind the
    // inventory's icons a moved item would shift every row and rebind the whole book.
    if let ScreenLayout::Personal { book } | ScreenLayout::Workbench { book } = layout {
        recipe_book::book_data(source, &mut data, frame, icons, book, book_cache);
    }
    let [inventory, hotbar] = player_rows(player_runtime, frame, icons);
    data.set_collection("inventory_items", inventory);
    data.set_collection("hotbar_items", hotbar);
    let mut cells = Cells {
        names: frame.item_names,
        icons,
    };
    match layout {
        ScreenLayout::Personal { .. } | ScreenLayout::Workbench { .. } => {
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
                        frame.crafting_icons.get(index).copied().flatten(),
                        None,
                    )
                })
                .collect();
            data.set_collection("crafting_input_items", grid);
            let output = match frame.crafting_output {
                Some((icon, stack)) => cells.cell(Some(stack), *icon, None),
                None => cells.cell(None, None, None),
            };
            data.set_collection("crafting_output_items", vec![output]);
            let armor = (0..4u8)
                .map(|slot| {
                    let stack = ledger.target_stack(InventoryTarget::Armor(slot));
                    cells
                        .cell(
                            stack,
                            frame.armor_icons[usize::from(slot)],
                            frame.armor_durability[usize::from(slot)],
                        )
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
            if furnace_book::active(player_runtime) {
                furnace_book::data(source, frame, &mut data, cells.icons, furnace_cache);
            }
            for (collection, addressed) in kind.collections {
                let shown =
                    container_data::collection_len(player_runtime, collection, addressed.len());
                let items = addressed[..shown]
                    .iter()
                    .map(|cell| {
                        let (stack, icon, durability) = station_cell(player_runtime, frame, *cell);
                        let item = cells.cell(stack, icon, durability);
                        container_data::decorate(source, collection, stack.is_none(), item)
                    })
                    .collect();
                data.set_collection(*collection, items);
            }
            if let Some(window) = ledger.window_kind() {
                container_data::station_globals(source, &mut data, window);
                container_data::station_controls(source, &mut data, frame, window);
            }
        }
        ScreenLayout::Book => {
            if let Some(book) = &source.screen().book {
                book_screen::book_data(&mut data, book);
            }
        }
    }
    data
}

#[cfg(test)]
mod anvil_output_tests;
#[cfg(test)]
mod empty_cells_tests;
#[cfg(test)]
mod press_drop_tests;

/// The stack, icon, and durability a station cell shows.
fn station_cell<'a>(
    player_runtime: &'a player_state::PlayerState,
    frame: &ContainerFrame<'a>,
    cell: Cell,
) -> (Option<&'a NetworkItemStack>, Option<IconRef>, Option<f32>) {
    let ledger = player_runtime.inventory.ledger();
    match cell {
        Cell::Storage(slot) => {
            let index = usize::from(slot);
            (
                ledger.furnace_visual_stack(slot),
                frame.storage_icons.get(index).copied().flatten(),
                frame.storage_durability.get(index).copied().flatten(),
            )
        }
        Cell::Ui(slot) => {
            let index = usize::from(slot);
            (
                ledger.target_stack(InventoryTarget::Craft(slot)),
                frame.ui_icons.get(index).copied().flatten(),
                frame.ui_durability.get(index).copied().flatten(),
            )
        }
        Cell::Output => {
            let index = usize::from(protocol::CREATED_OUTPUT_SLOT);
            match (ledger.created_output_stack(), frame.recipe_output) {
                // The anvil's result is the client's prediction until the server states one.
                (None, Some(output))
                    if ledger.window_kind() == Some(protocol::WindowKind::Anvil) =>
                {
                    (Some(&output.stack), output.icon, output.durability)
                }
                (stack, _) => (
                    stack,
                    frame.ui_icons.get(index).copied().flatten(),
                    frame.ui_durability.get(index).copied().flatten(),
                ),
            }
        }
    }
}

/// The button the pack's close (X) controls press, as `$close_button_to_button_id`.
const CLOSE_BUTTON: &str = "button.menu_exit";

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
        // Desktop screens show their close (X) button; it presses `CLOSE_BUTTON`.
        ("#close_button_visible", true),
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
#[path = "containers/texture_metadata_tests.rs"]
mod texture_metadata_tests;

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
                false,
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
