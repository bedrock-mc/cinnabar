//! Icons for server-defined items, packed onto the last dynamic UI page.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use render_model::UiTexturePage;
use ui::UiMesh;

use {
    super::{
        UiPresentationRuntime, dynamic_textures,
        gui_models::{IconKey, icon_key, ordinary_cube_sheet, sheet_faces},
        item_gui,
    },
    ui::IconRef,
};

/// Largest icon side kept as-is; larger sources are reduced to fit.
pub const MAX_SESSION_ICON_SIDE: u32 = 64;
const MIN_PAGE_SIDE: u32 = render_model::UI_DYNAMIC_PAGE_SIDE;
type IconRefs = HashMap<Arc<str>, BTreeMap<u32, IconRef>>;
const GUTTER: u32 = 1;
const MAX_LOGGED_MISSES: usize = 512;

/// One server item icon in straight-alpha RGBA8.
#[derive(Debug)]
pub struct SessionIcon {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// The session's server item icons; later entries for an identifier are ignored.
#[derive(Debug, Default)]
pub struct SessionIcons {
    pub icons: Vec<SessionIcon>,
    /// Six-face sheets (`assets::BLOCK_ITEM_SHEET_SIZE`) for custom held cubes.
    pub block_sheets: Vec<SessionIcon>,
    /// Admitted face material flags for each custom block item's sheet.
    pub block_material_flags: BTreeMap<Arc<str>, u32>,
    /// Why an item's icon key did not resolve to an image, for diagnostics.
    pub misses: HashMap<Arc<str>, Box<str>>,
}

/// The packed page and lookups for the icons last seen on the UI runtime.
#[derive(Default)]
pub(super) struct SessionIconPage {
    source: Option<Arc<SessionIcons>>,
    refs: IconRefs,
    /// GUI cubes of custom block items, keyed by their flat thumbnail as vanilla GUI models are.
    pub(super) models: BTreeMap<IconKey, Arc<UiMesh>>,
    pub(super) page: Option<UiTexturePage>,
    generation: u64,
    /// A newer set packing on a worker; the installed set draws until it lands.
    packing: Option<Packing>,
}

/// An icon set packing off the frame, and where its page lands.
struct Packing {
    source: Arc<SessionIcons>,
    done: crossbeam_channel::Receiver<Option<PackedIcons>>,
}

/// Icons whose padded area fits one minimal page pack inline; larger sets pack on a worker,
/// since one pass cannot stop at a frame budget.
const MAX_INLINE_PACK_TEXELS: u64 = MIN_PAGE_SIDE as u64 * MIN_PAGE_SIDE as u64;

impl UiPresentationRuntime {
    /// Changes whenever session icons are replaced; pixels copied from them are stale after.
    pub fn session_icon_generation(&self) -> u64 {
        self.session_icons.generation
    }

    /// Vanilla crossbows route nonzero animation frames to the pulling
    /// atlas. This identity is shared by inventory cells and actual dropped sprites.
    pub fn item_icon_key<'a>(
        identifier: &'a str,
        metadata: u32,
        charged_projectile: Option<&str>,
        animation_frame: Option<u32>,
    ) -> (&'a str, u32) {
        if identifier != "minecraft:crossbow" {
            return (identifier, metadata);
        }
        let frame = animation_frame.unwrap_or_else(|| {
            inventory::crossbow_animation_frame(None, 0, charged_projectile, false)
        });
        if frame == 0 {
            (identifier, metadata)
        } else {
            ("minecraft:crossbow_pulling", frame - 1)
        }
    }

    /// Resolves an item identity to its icon: a server icon for this session
    /// first, then the vanilla atlas. Unknown items keep only the slot frame.
    pub fn item_icon(&self, identifier: &str, metadata: u32) -> Option<IconRef> {
        if let Some(icon) = self
            .session_icons
            .refs
            .get(identifier)
            .and_then(|variants| variants.get(&metadata).or_else(|| variants.get(&0)))
        {
            return Some(*icon);
        }
        let vanilla = self
            .icon_catalog
            .as_ref()
            .and_then(|catalog| catalog.lookup_index(identifier, metadata))
            .and_then(|sprite| self.icon_refs.as_deref()?.get(sprite).copied());
        if vanilla.is_none() {
            self.note_missing_icon(identifier, metadata);
        }
        vanilla
    }

    /// Logs the hotbar's identifiers and icon presence whenever they change.
    pub fn note_hotbar(&mut self, slots: [Option<(Arc<str>, bool)>; 9]) {
        if slots == self.logged_hotbar {
            return;
        }
        let shown = slots
            .iter()
            .map(|slot| match slot {
                Some((identifier, true)) => identifier.to_string(),
                Some((identifier, false)) => format!("{identifier} (no icon)"),
                None => "-".to_owned(),
            })
            .collect::<Vec<_>>();
        bevy::log::info!(slots = ?shown, "hotbar changed");
        self.logged_hotbar = slots;
    }

    /// Logs once per identifier why no icon resolved: the session, pack and vanilla lookups.
    fn note_missing_icon(&self, identifier: &str, metadata: u32) {
        let Ok(mut seen) = self.missing_icons.lock() else {
            return;
        };
        if seen.len() >= MAX_LOGGED_MISSES || !seen.insert(identifier.to_owned()) {
            return;
        }
        let session = self.session_icons.source.as_ref().map_or_else(
            || "no session icons (no server pack icons)".to_owned(),
            |icons| {
                icons.misses.get(identifier).map_or_else(
                    || "not among the pack stack's item icon keys".to_owned(),
                    |reason| reason.to_string(),
                )
            },
        );
        let vanilla = if self.icon_catalog.is_none() {
            "vanilla icon carrier not loaded"
        } else {
            "not in the vanilla icon catalog"
        };
        bevy::log::info!(
            identifier,
            metadata,
            custom = !identifier.starts_with("minecraft:"),
            "no icon for item: session/pack: {session}; vanilla: {vanilla}"
        );
    }
}

/// Repacks the page when the runtime's icon set changes identity. A large set packs on a
/// worker while the installed set keeps drawing, and installs on the frame it lands.
pub(super) fn observe(runtime: &mut UiPresentationRuntime, icons: Option<&Arc<SessionIcons>>) {
    let unchanged = match (&runtime.session_icons.source, icons) {
        (Some(current), Some(next)) => Arc::ptr_eq(current, next),
        (None, None) => true,
        _ => false,
    };
    if unchanged {
        runtime.session_icons.packing = None;
        return;
    }
    let Some(icons) = icons else {
        install(runtime, None, None);
        return;
    };
    if packed_texels(icons) <= MAX_INLINE_PACK_TEXELS {
        install(runtime, Some(icons), pack(icons, 0));
        return;
    }
    let packing = match runtime.session_icons.packing.take() {
        Some(packing) if Arc::ptr_eq(&packing.source, icons) => packing,
        _ => {
            let (sender, done) = crossbeam_channel::bounded(1);
            let source = Arc::clone(icons);
            rayon::spawn(move || {
                let _ = sender.send(pack(&source, 0));
            });
            Packing {
                source: Arc::clone(icons),
                done,
            }
        }
    };
    match packing.done.try_recv() {
        Ok(packed) => install(runtime, Some(icons), packed),
        Err(crossbeam_channel::TryRecvError::Empty) => {
            runtime.session_icons.packing = Some(packing)
        }
        // A worker that died packed nothing; the set installs without a page.
        Err(crossbeam_channel::TryRecvError::Disconnected) => install(runtime, Some(icons), None),
    }
}

/// Installs `icons` with their page packed at relative page 0, onto the session icon page.
fn install(
    runtime: &mut UiPresentationRuntime,
    icons: Option<&Arc<SessionIcons>>,
    packed: Option<PackedIcons>,
) {
    let page_index =
        (runtime.textures.dynamic_start() + dynamic_textures::SESSION_ICON_PAGE) as u16;
    let packed = icons
        .zip(packed)
        .map(|(icons, packed)| (icons, packed.on_page(page_index)));
    let models = packed
        .as_ref()
        .map(|(icons, packed)| block_cubes(icons, packed))
        .unwrap_or_default();
    let (page, refs) = packed.map_or((None, HashMap::new()), |(_, packed)| {
        (Some(packed.page), packed.refs)
    });
    runtime.session_icons = SessionIconPage {
        source: icons.cloned(),
        refs,
        models,
        page,
        generation: runtime.session_icons.generation.wrapping_add(1),
        packing: None,
    };
    dynamic_textures::rebuild(runtime);
}

/// Texels the set's valid icons and sheets cover with their gutters.
fn packed_texels(icons: &SessionIcons) -> u64 {
    icons
        .icons
        .iter()
        .chain(&icons.block_sheets)
        .filter(|icon| icon.width <= MAX_SESSION_ICON_SIDE && icon.height <= MAX_SESSION_ICON_SIDE)
        .map(|icon| u64::from(icon.width + GUTTER * 2) * u64::from(icon.height + GUTTER * 2))
        .sum()
}

/// The session page's pixels and where each icon and block sheet landed on it.
struct PackedIcons {
    page: UiTexturePage,
    refs: IconRefs,
    sheets: HashMap<Arc<str>, IconRef>,
}

impl PackedIcons {
    /// The same placements on UI page `page`.
    fn on_page(mut self, page: u16) -> Self {
        for icon in self
            .refs
            .values_mut()
            .flat_map(BTreeMap::values_mut)
            .chain(self.sheets.values_mut())
        {
            icon.page = page;
        }
        self
    }
}

/// Each placed opaque block sheet's GUI cube, keyed by its item's flat thumbnail: the cube
/// replaces that sprite wherever JSON-UI draws the item, as vanilla block items' cubes do.
fn block_cubes(icons: &SessionIcons, packed: &PackedIcons) -> BTreeMap<IconKey, Arc<UiMesh>> {
    let mut models = BTreeMap::new();
    for sheet in &icons.block_sheets {
        let (Some(placed), Some(thumbnail)) = (
            packed.sheets.get(&sheet.identifier),
            packed
                .refs
                .get(&sheet.identifier)
                .and_then(|variants| variants.get(&sheet.metadata)),
        ) else {
            continue;
        };
        if !ordinary_cube_sheet(&sheet.rgba8) {
            continue;
        }
        if let Some(mesh) = item_gui::cube(sheet_faces(*placed)) {
            models.insert(icon_key(*thumbnail), mesh);
        }
    }
    models
}

/// Shelf-packs icons and block sheets with a replicated gutter; those that do not fit, and
/// sheets not shaped as `assets::BLOCK_ITEM_SHEET_SIZE`, are left out.
fn pack(icons: &SessionIcons, page_index: u16) -> Option<PackedIcons> {
    #[cfg(test)]
    PACKS.with(|packs| packs.set(packs.get() + 1));
    let sheet_size = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    // Tallest first keeps shelves dense; ties keep input order.
    // Invalid entries and later duplicates are dropped before sorting so they cannot size the page.
    let mut seen = std::collections::HashSet::new();
    let mut ordered = icons
        .icons
        .iter()
        .map(|icon| (icon, false))
        .chain(
            icons
                .block_sheets
                .iter()
                .filter(|sheet| [sheet.width, sheet.height] == sheet_size)
                .map(|sheet| (sheet, true)),
        )
        .filter(|(icon, _)| {
            icon.width > 0
                && icon.height > 0
                && icon.width <= MAX_SESSION_ICON_SIDE
                && icon.height <= MAX_SESSION_ICON_SIDE
                && icon.rgba8.len() == (icon.width * icon.height * 4) as usize
        })
        .filter(|(icon, sheet)| seen.insert((Arc::clone(&icon.identifier), icon.metadata, *sheet)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(icon, _)| std::cmp::Reverse(icon.height));
    let side = page_side(ordered.iter().map(|(icon, _)| *icon));
    let mut rgba8 = vec![0u8; (side * side * 4) as usize];
    let mut refs: IconRefs = HashMap::new();
    let mut sheets = HashMap::new();
    let (mut cursor, mut row_height) = ([0u32; 2], 0u32);
    for (icon, sheet) in ordered {
        let padded = [icon.width + GUTTER * 2, icon.height + GUTTER * 2];
        let placed = if sheet {
            sheets.contains_key(&icon.identifier)
        } else {
            refs.get(&icon.identifier)
                .is_some_and(|variants| variants.contains_key(&icon.metadata))
        };
        if placed
            || icon.width == 0
            || icon.height == 0
            || icon.width > MAX_SESSION_ICON_SIDE
            || icon.height > MAX_SESSION_ICON_SIDE
            || icon.rgba8.len() != (icon.width * icon.height * 4) as usize
        {
            continue;
        }
        if cursor[0] + padded[0] > side {
            cursor = [0, cursor[1] + row_height];
            row_height = 0;
        }
        if cursor[1] + padded[1] > side {
            continue;
        }
        for y in 0..padded[1] {
            let source_y = y.saturating_sub(GUTTER).min(icon.height - 1);
            for x in 0..padded[0] {
                let source_x = x.saturating_sub(GUTTER).min(icon.width - 1);
                let source = ((source_y * icon.width + source_x) * 4) as usize;
                let target = (((cursor[1] + y) * side + cursor[0] + x) * 4) as usize;
                rgba8[target..target + 4].copy_from_slice(&icon.rgba8[source..source + 4]);
            }
        }
        let [left, top] = [cursor[0] + GUTTER, cursor[1] + GUTTER];
        let placement = IconRef {
            page: page_index,
            uv: [
                left as u16,
                top as u16,
                (left + icon.width) as u16,
                (top + icon.height) as u16,
            ],
            glint: false,
        };
        if sheet {
            sheets.insert(Arc::clone(&icon.identifier), placement);
        } else {
            refs.entry(Arc::clone(&icon.identifier))
                .or_default()
                .insert(icon.metadata, placement);
        }
        cursor[0] += padded[0];
        row_height = row_height.max(padded[1]);
    }
    let page = UiTexturePage::owned([side, side], rgba8.into()).ok()?;
    Some(PackedIcons { page, refs, sheets })
}

/// Grows the reserved page to the smallest bounded shelf layout containing the icons.
fn page_side<'a>(icons: impl Iterator<Item = &'a SessionIcon> + Clone) -> u32 {
    let mut side = MIN_PAGE_SIDE;
    loop {
        let (mut x, mut y, mut row) = (0, 0, 0);
        for icon in icons.clone() {
            if icon.width > MAX_SESSION_ICON_SIDE || icon.height > MAX_SESSION_ICON_SIDE {
                continue;
            }
            let (width, height) = (icon.width + GUTTER * 2, icon.height + GUTTER * 2);
            if x + width > side {
                x = 0;
                y += row;
                row = 0;
            }
            x += width;
            row = row.max(height);
        }
        if y + row <= side || side == render_model::MAX_UI_TEXTURE_SIDE {
            return side;
        }
        side = (side * 2).min(render_model::MAX_UI_TEXTURE_SIDE);
    }
}

#[cfg(test)]
thread_local! {
    /// Icon pages packed on this thread, so tests can tell the frame from a worker.
    static PACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
