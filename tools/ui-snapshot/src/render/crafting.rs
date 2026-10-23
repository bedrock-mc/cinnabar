//! Production recipe-book painting and hit routing over original authoritative fixture inputs.

mod session;
mod source;

use std::sync::Arc;

use assets::RuntimeLangCatalog;
use inventory::inventory_ledger::InventoryGestureError;
use jsonui_editor::raster::Page;
use player_state::PlayerState;
use ui::IconRef;
use ui_engine::{
    engine::{FormEngine, screen::PaintFrame},
    frame::EngineFrame,
};
use ui_screens::{
    container_actions::InventoryCellHit,
    container_source::{ContainerFrame, ContainerSource},
    containers::{ContainerInput, ContainerScreen, ScreenLayout},
    inventory_capture::{capture_contents, capture_decorations},
};

use crate::carriers::Error;

const ICON_SIZE: u16 = 16;
const LAYOUT: ScreenLayout = ScreenLayout::Personal { book: true };

/// Adds an original geometric palette; no game item images are embedded or downloaded.
pub fn icons(pages: &mut Vec<Page>) -> Result<[IconRef; session::ITEMS.len()], Error> {
    let page = u16::try_from(pages.len()).map_err(|error| error.to_string())?;
    let width = ICON_SIZE * session::ITEMS.len() as u16;
    let mut rgba = vec![0; usize::from(width) * usize::from(ICON_SIZE) * 4];
    let icons = std::array::from_fn(|index| {
        let color = session::ITEMS[index].color;
        for y in 1..ICON_SIZE - 1 {
            for x in 1..ICON_SIZE - 1 {
                let pixel = (usize::from(y) * usize::from(width)
                    + index * usize::from(ICON_SIZE)
                    + usize::from(x))
                    * 4;
                let shade = if x == 1 || y == 1 || (x + y) % 5 == 0 {
                    30
                } else {
                    0
                };
                rgba[pixel..pixel + 4].copy_from_slice(&[
                    color[0].saturating_add(shade),
                    color[1].saturating_add(shade),
                    color[2].saturating_add(shade),
                    255,
                ]);
            }
        }
        IconRef {
            page,
            uv: [
                index as u16 * ICON_SIZE,
                0,
                (index as u16 + 1) * ICON_SIZE,
                ICON_SIZE,
            ],
            glint: false,
        }
    });
    pages.push(Page {
        repeat_content: None,
        width: width.into(),
        height: ICON_SIZE.into(),
        rgba: rgba.into(),
    });
    Ok(icons)
}

/// Selects the painted recipe using its actual hit map, then repaints the ledger's result.
/// An invalid allocation is retained as the base revision's refused-click state.
pub fn paint(
    engine: &FormEngine,
    lang: &RuntimeLangCatalog,
    icons: &[IconRef; session::ITEMS.len()],
    selected: bool,
    block_identity: i32,
    frame: &mut PaintFrame<'_>,
) -> Result<(), Error> {
    let mut player = session::player(block_identity)?;
    let mut screen = ContainerScreen::default();
    let first = draw(
        engine,
        lang,
        icons,
        &player,
        &mut screen,
        None,
        None,
        None,
        frame,
    )?;
    let pointer = first
        .hits
        .iter()
        .find_map(|region| {
            let left = region.rect.x.max(region.clip.x);
            let top = region.rect.y.max(region.clip.y);
            let right = (region.rect.x + region.rect.w).min(region.clip.x + region.clip.w);
            let bottom = (region.rect.y + region.rect.h).min(region.clip.y + region.clip.h);
            if left >= right || top >= bottom {
                return None;
            }
            let point = [((left + right) / 2.0) as f32, ((top + bottom) / 2.0) as f32];
            matches!(
                LAYOUT.hit(&first, point),
                Some(InventoryCellHit::RecipeBook(0))
            )
            .then_some(point)
        })
        .ok_or_else(|| Error::Invalid("craft fixture recipe has no usable hit region".into()))?;
    frame.now = super::CAPTURE_SECONDS;
    frame.nodes.clear();
    *frame.next = 1;
    let ready = draw(
        engine,
        lang,
        icons,
        &player,
        &mut screen,
        Some(&first),
        Some(pointer),
        None,
        frame,
    )?;
    if selected {
        let Some(InventoryCellHit::RecipeBook(index)) = LAYOUT.hit(&ready, pointer) else {
            return Err(Error::Invalid(
                "craft fixture recipe hit changed while settling".into(),
            ));
        };
        let recipe = player
            .inventory
            .book_recipes(false, false, usize::from(index), 1)
            .pop()
            .ok_or_else(|| Error::Invalid("craft fixture selected recipe is absent".into()))?;
        match player.inventory.begin_book_craft(&recipe.recipe, false) {
            Ok(_) | Err(InventoryGestureError::InvalidRequest) => {}
            Err(error) => {
                return Err(Error::Invalid(format!(
                    "craft fixture click failed: {error:?}"
                )));
            }
        }
        frame.nodes.clear();
        *frame.next = 1;
        draw(
            engine,
            lang,
            icons,
            &player,
            &mut screen,
            Some(&ready),
            Some(pointer),
            None,
            frame,
        )?;
    }
    Ok(())
}

/// Picks up a fixture stack through the ledger, then paints its half-worn cursor decoration.
pub fn paint_cursor(
    engine: &FormEngine,
    lang: &RuntimeLangCatalog,
    icons: &[IconRef; session::ITEMS.len()],
    frame: &mut PaintFrame<'_>,
) -> Result<(), Error> {
    let mut player = session::player(0)?;
    player
        .inventory
        .ledger_mut()
        .begin_take_count(protocol::HOTBAR_SLOT_COUNT, 1)
        .map_err(|error| Error::Invalid(format!("cursor fixture pickup failed: {error:?}")))?;
    draw(
        engine,
        lang,
        icons,
        &player,
        &mut ContainerScreen::default(),
        None,
        Some([260.0, 150.0]),
        Some(0.5),
        frame,
    )?;
    Ok(())
}

/// Projects icons and counts from the ledger and leaves craftability to the container painter.
#[expect(
    clippy::too_many_arguments,
    reason = "fixture paint adapter borrows each owner independently"
)]
fn draw(
    engine: &FormEngine,
    lang: &RuntimeLangCatalog,
    icons: &[IconRef; session::ITEMS.len()],
    player: &PlayerState,
    screen: &mut ContainerScreen,
    previous: Option<&EngineFrame>,
    pointer: Option<[f32; 2]>,
    cursor_durability: Option<f32>,
    frame: &mut PaintFrame<'_>,
) -> Result<EngineFrame, Error> {
    let icon = |stack: &protocol::NetworkItemStack| {
        session::ITEMS
            .iter()
            .position(|item| item.id == stack.network_id)
            .map(|index| icons[index])
    };
    let ledger = player.inventory.ledger();
    let contents = capture_contents(
        ledger,
        true,
        || player.inventory.crafting_match(),
        |stack, _| icon(stack),
        |stack| {
            session::ITEMS
                .iter()
                .find(|item| item.id == stack.network_id)
                .map(|item| item.label.to_owned())
        },
    );
    let (durability, ui_icons) = capture_decorations(ledger, |_, _| None, icon);
    let groups = Default::default();
    let scrolls = Default::default();
    let source = source::Source::new(player, lang, &groups, &scrolls);
    let entries: Vec<_> = source
        .book_entries()
        .iter()
        .map(|entry| icon(&entry.stack()))
        .collect();
    let furnace_entries = Arc::from([]);
    let items = ContainerFrame {
        item_names: &contents.item_names,
        inventory_icons: &contents.inventory_icons.0,
        player_durability: &durability.player,
        hotbar_durability: (&durability.player[..usize::from(protocol::HOTBAR_SLOT_COUNT)])
            .try_into()
            .unwrap(),
        storage_icons: &contents.storage_icons.0,
        storage_durability: &durability.storage,
        ui_icons: &ui_icons,
        ui_durability: &durability.ui,
        crafting: &contents.crafting,
        armor_icons: &[None; 4],
        armor_durability: &[None; 4],
        offhand_icon: None,
        offhand_durability: None,
        cursor_icon: ledger.cursor_stack().and_then(icon),
        cursor_durability,
        recipe_icons: &[],
        recipe_output: None,
        book_entries: &entries,
        furnace_entries: &furnace_entries,
    };
    screen
        .paint(
            engine,
            &source,
            &items,
            ContainerInput {
                layout: LAYOUT,
                title: if cursor_durability.is_some() {
                    "Cursor durability"
                } else {
                    "Overlapping ingredients"
                },
                previous,
                pointer,
                tooltip: None,
                clocks: &scrolls,
                id_aux: &[],
            },
            frame,
        )
        .ok_or_else(|| Error::Invalid("craft fixture inventory screen did not resolve".into()))
}
