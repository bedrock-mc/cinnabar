//! Icons for server-defined items, packed onto the last dynamic UI page.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock},
};

use assets::texture::policy::{TexturePolicy, device_policy};
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

const MIN_PAGE_SIDE: u32 = render_model::UI_DYNAMIC_PAGE_SIDE;
type IconRefs = HashMap<Arc<str>, BTreeMap<u32, IconRef>>;
const GUTTER: u32 = 1;
const MAX_LOGGED_MISSES: usize = 512;

mod held;

/// Resolves session overrides before vanilla artwork, retaining the variant-zero fallback.
pub(super) fn lookup_icon(
    session: &SessionIconPage,
    catalog: Option<&assets::RuntimeIconCatalog>,
    refs: Option<&[IconRef]>,
    identifier: &str,
    metadata: u32,
) -> Option<IconRef> {
    session.icon(identifier, metadata).or_else(|| {
        let sprite = catalog?.lookup_index(identifier, metadata)?;
        refs?.get(sprite).copied()
    })
}

/// One server item icon in straight-alpha RGBA8.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionIcon {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// The session's server item icons; later entries for an identifier are ignored.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionIcons {
    pub icons: Vec<SessionIcon>,
    /// Material sheets (`assets::BLOCK_ITEM_SHEET_SIZE`) for custom block items.
    pub block_sheets: Vec<SessionIcon>,
    /// Exact held models with UVs relative to the item's matching material sheet.
    pub block_models: BTreeMap<Arc<str>, Arc<[render_model::ActorRigVertex]>>,
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
    pub(super) sheets: IconRefs,
    /// Only admitted sprites get meshes; replacing the page replaces this lazy cache.
    held_sprites: BTreeMap<IconKey, HeldSprite>,
    held_meshes: Mutex<held::HeldMeshes>,
    /// GUI cubes of custom block items, keyed by their flat thumbnail as vanilla GUI models are.
    pub(super) models: BTreeMap<IconKey, Arc<UiMesh>>,
    pub(super) page: Option<UiTexturePage>,
    generation: u64,
    /// A newer set packing on a worker; the installed set draws until it lands.
    packing: Option<Packing>,
}

impl SessionIconPage {
    /// The original pixels of the admitted exact variant or its variant-zero fallback.
    pub(super) fn sprite(&self, identifier: &str, metadata: u32) -> Option<&SessionIcon> {
        let source = self.icon(identifier, metadata)?;
        let held = self.held_sprites.get(&icon_key(source))?;
        self.source.as_ref()?.icons.get(held.index)
    }

    /// Converts admitted pixels once per installed page for the compatibility CPU preview.
    pub(super) fn sprite_pixels(&self, identifier: &str, metadata: u32) -> Option<Arc<[u8]>> {
        let source = self.icon(identifier, metadata)?;
        let held = self.held_sprites.get(&icon_key(source))?;
        let sprite = self.source.as_ref()?.icons.get(held.index)?;
        Some(Arc::clone(
            held.pixels.get_or_init(|| Arc::from(&*sprite.rgba8)),
        ))
    }

    /// Shares the inventory cell's region and lazily extrudes its admitted sprite's alpha edges.
    pub(super) fn held_sprite(
        &self,
        identifier: &str,
        metadata: u32,
    ) -> Option<(IconRef, Arc<[render_model::ActorRigVertex]>)> {
        let source = self.icon(identifier, metadata)?;
        let held = self.held_sprites.get(&icon_key(source))?;
        if held.rejected.get().is_some() {
            return None;
        }
        let mut cache = self.held_meshes.lock().ok()?;
        if let Some(vertices) = cache.get(icon_key(source)) {
            return Some((source, vertices));
        }
        let sprite = self.source.as_ref()?.icons.get(held.index)?;
        let vertices = render_model::held_sprite_vertices(
            sprite.width as usize,
            sprite.height as usize,
            &sprite.rgba8,
            [0.0, 0.0, 1.0, 1.0],
        )
        .filter(|vertices| UiMesh::check_size(vertices.len(), vertices.len(), 1).is_ok());
        let Some(vertices) = vertices else {
            let _ = held.rejected.set(());
            return None;
        };
        let vertices: Arc<[render_model::ActorRigVertex]> = vertices.into();
        cache.insert(icon_key(source), Arc::clone(&vertices))?;
        Some((source, vertices))
    }

    /// Shares exact block geometry with the admitted material sheet for player previews.
    pub(super) fn held_block(
        &self,
        identifier: &str,
        metadata: u32,
    ) -> Option<(IconRef, Arc<[render_model::ActorRigVertex]>)> {
        let source = self.sheets.get(identifier)?;
        let source = *source.get(&metadata).or_else(|| source.get(&0))?;
        let vertices = self.source.as_ref()?.block_models.get(identifier)?.clone();
        Some((source, vertices))
    }

    /// Returns a session override for the requested variant, then variant zero.
    fn icon(&self, identifier: &str, metadata: u32) -> Option<IconRef> {
        self.refs
            .get(identifier)
            .and_then(|variants| variants.get(&metadata).or_else(|| variants.get(&0)))
            .copied()
    }
}

/// A source index and mesh owned by the installed session page's generation.
struct HeldSprite {
    index: usize,
    pixels: OnceLock<Arc<[u8]>>,
    rejected: OnceLock<()>,
}

/// An icon set packing off the frame, and where its page lands.
struct Packing {
    source: Arc<SessionIcons>,
    done: crossbeam_channel::Receiver<Option<PackedIcons>>,
}

/// Inline packing bounds both copied texels and the allocated page to one minimal page.
const MAX_INLINE_PACK_TEXELS: u64 = MIN_PAGE_SIDE as u64 * MIN_PAGE_SIDE as u64;

impl UiPresentationRuntime {
    /// Changes whenever session icons are replaced; pixels copied from them are stale after.
    pub fn session_icon_generation(&self) -> u64 {
        self.session_icons.generation
    }

    /// Admitted crossbow drawing roles share the pulling atlas across cells and dropped sprites.
    pub fn item_icon_key(
        identifier: &str,
        metadata: u32,
        animation: assets::item_use::ItemUseAnimation,
        charged_projectile: assets::item_use::ItemProjectileRole,
        animation_frame: Option<u32>,
    ) -> (&str, u32) {
        if animation != assets::item_use::ItemUseAnimation::Crossbow {
            return (identifier, metadata);
        }
        let frame = animation_frame.unwrap_or_else(|| {
            inventory::crossbow_animation_frame(None, 0, Some(charged_projectile), false)
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
        let icon = lookup_icon(
            &self.session_icons,
            self.icon_catalog.as_deref(),
            self.icon_refs.as_deref(),
            identifier,
            metadata,
        );
        if icon.is_none() {
            self.note_missing_icon(identifier, metadata);
        }
        icon
    }

    /// Returns the session override for this item variant, falling back to variant zero.
    pub(super) fn session_item_icon(&self, identifier: &str, metadata: u32) -> Option<IconRef> {
        self.session_icons.icon(identifier, metadata)
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
        tracing::info!(slots = ?shown, "hotbar changed");
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
        // Debug level: opening the creative search can miss hundreds at once, and every line
        // costs a synchronous console write in browsers.
        tracing::debug!(
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
    let packing = match runtime.session_icons.packing.take() {
        Some(packing) if Arc::ptr_eq(&packing.source, icons) => packing,
        _ => {
            if inline_pack_fits(icons) {
                install(runtime, Some(icons), pack(icons, 0));
                return;
            }
            let (sender, done) = crossbeam_channel::bounded(1);
            let source = Arc::clone(icons);
            let work = move || {
                let _ = sender.send(pack(&source, 0));
            };
            #[cfg(not(target_arch = "wasm32"))]
            rayon::spawn(work);
            #[cfg(target_arch = "wasm32")]
            crate::browser_jobs::submit(crate::browser_jobs::Lane::Icons, work);
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
        (runtime.textures.dynamic_start() + ui_engine::pages::SESSION_ICON_PAGE) as u16;
    let packed = icons
        .zip(packed)
        .map(|(icons, packed)| (icons, packed.on_page(page_index)));
    let models = packed
        .as_ref()
        .map(|(icons, packed)| block_cubes(icons, packed))
        .unwrap_or_default();
    let (page, refs, sheets) = packed
        .map_or((None, HashMap::new(), HashMap::new()), |(_, packed)| {
            (Some(packed.page), packed.refs, packed.sheets)
        });
    let mut held_sprites = BTreeMap::new();
    if let Some(icons) = icons {
        let policy = device_policy();
        for (index, icon) in icons
            .icons
            .iter()
            .enumerate()
            .filter(|(_, icon)| valid_icon(icon, policy))
        {
            if let Some(source) = refs
                .get(&icon.identifier)
                .and_then(|variants| variants.get(&icon.metadata))
            {
                held_sprites
                    .entry(icon_key(*source))
                    .or_insert_with(|| HeldSprite {
                        index,
                        pixels: OnceLock::new(),
                        rejected: OnceLock::new(),
                    });
            }
        }
    }
    runtime.session_icons = SessionIconPage {
        source: icons.cloned(),
        refs,
        sheets,
        held_sprites,
        held_meshes: Mutex::default(),
        models,
        page,
        generation: runtime.session_icons.generation.wrapping_add(1),
        packing: None,
    };
    dynamic_textures::rebuild(runtime);
}

/// Texels the set's valid icons and sheets cover with their gutters.
fn packed_texels(icons: &SessionIcons) -> u64 {
    let policy = device_policy();
    icons
        .icons
        .iter()
        .chain(&icons.block_sheets)
        .filter(|icon| valid_icon(icon, policy))
        .map(|icon| u64::from(icon.width + GUTTER * 2) * u64::from(icon.height + GUTTER * 2))
        .sum()
}

/// Bounds page allocation and hashing using the same shelf plan as the eventual pack.
fn inline_pack_fits(icons: &SessionIcons) -> bool {
    if packed_texels(icons) > MAX_INLINE_PACK_TEXELS {
        return false;
    }
    let policy = device_policy();
    let ordered = ordered_icons(icons, policy);
    page_side(ordered.iter().map(|(icon, _)| *icon), policy)
        .is_none_or(|side| u64::from(side) * u64::from(side) <= MAX_INLINE_PACK_TEXELS)
}

/// The session page's pixels and where each icon and block sheet landed on it.
struct PackedIcons {
    page: UiTexturePage,
    refs: IconRefs,
    sheets: IconRefs,
}

impl PackedIcons {
    /// The same placements on UI page `page`.
    fn on_page(mut self, page: u16) -> Self {
        for icon in self
            .refs
            .values_mut()
            .flat_map(BTreeMap::values_mut)
            .chain(self.sheets.values_mut().flat_map(BTreeMap::values_mut))
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
            packed
                .sheets
                .get(&sheet.identifier)
                .and_then(|variants| variants.get(&sheet.metadata)),
            packed
                .refs
                .get(&sheet.identifier)
                .and_then(|variants| variants.get(&sheet.metadata)),
        ) else {
            continue;
        };
        if icons.block_models.contains_key(&sheet.identifier) {
            continue;
        }
        if icons
            .block_material_flags
            .get(&sheet.identifier)
            .is_some_and(|flags| flags & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING != 0)
            || !ordinary_cube_sheet(&sheet.rgba8)
        {
            continue;
        }
        if let Some(mesh) = item_gui::cube(sheet_faces(*placed)) {
            models.insert(icon_key(*thumbnail), mesh);
        }
    }
    models
}

/// Selects valid first occurrences and sorts them tallest first for every packing plan.
fn ordered_icons(icons: &SessionIcons, policy: TexturePolicy) -> Vec<(&SessionIcon, bool)> {
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
        .filter(|(icon, _)| valid_icon(icon, policy))
        .filter(|(icon, sheet)| seen.insert((Arc::clone(&icon.identifier), icon.metadata, *sheet)))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(icon, _)| std::cmp::Reverse(icon.height));
    ordered
}

/// Shelf-packs icons and block sheets with a replicated gutter; those that do not fit, and
/// sheets not shaped as `assets::BLOCK_ITEM_SHEET_SIZE`, are left out.
fn pack(icons: &SessionIcons, page_index: u16) -> Option<PackedIcons> {
    #[cfg(test)]
    PACKS.with(|packs| packs.set(packs.get() + 1));
    let policy = device_policy();
    let ordered = ordered_icons(icons, policy);
    let side = page_side(ordered.iter().map(|(icon, _)| *icon), policy)?;
    let mut rgba8 = vec![0u8; (side * side * 4) as usize];
    let mut refs: IconRefs = HashMap::new();
    let mut sheets: IconRefs = HashMap::new();
    let (mut cursor, mut row_height) = ([0u32; 2], 0u32);
    for (icon, sheet) in ordered {
        let padded = [icon.width + GUTTER * 2, icon.height + GUTTER * 2];
        let placements = if sheet { &mut sheets } else { &mut refs };
        if placements
            .get(&icon.identifier)
            .is_some_and(|variants| variants.contains_key(&icon.metadata))
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
            color_mask: None,
            page: page_index,
            uv: [
                left as u16,
                top as u16,
                (left + icon.width) as u16,
                (top + icon.height) as u16,
            ],
            glint: false,
        };
        placements
            .entry(Arc::clone(&icon.identifier))
            .or_default()
            .insert(icon.metadata, placement);
        cursor[0] += padded[0];
        row_height = row_height.max(padded[1]);
    }
    let page = UiTexturePage::owned([side, side], rgba8.into()).ok()?;
    Some(PackedIcons { page, refs, sheets })
}

/// Grows the reserved page to the smallest bounded shelf layout containing the icons.
fn page_side<'a>(
    icons: impl Iterator<Item = &'a SessionIcon> + Clone,
    policy: TexturePolicy,
) -> Option<u32> {
    let max_side = policy.max_side().min(render_model::MAX_UI_TEXTURE_SIDE);
    if max_side < MIN_PAGE_SIDE {
        return None;
    }
    let max_side = 1 << max_side.ilog2();
    let mut side = MIN_PAGE_SIDE;
    loop {
        let (mut x, mut y, mut row) = (0, 0, 0);
        for icon in icons.clone() {
            let (width, height) = (icon.width + GUTTER * 2, icon.height + GUTTER * 2);
            if x + width > side {
                x = 0;
                y += row;
                row = 0;
            }
            x += width;
            row = row.max(height);
        }
        let widths_fit = icons.clone().all(|icon| icon.width + GUTTER * 2 <= side);
        if (widths_fit && y + row <= side) || side == max_side {
            policy.page_bytes(side, side)?;
            return Some(side);
        }
        side = (side * 2).min(max_side);
    }
}

/// Validates source pixels and the padded physical extent before gutter arithmetic.
fn valid_icon(icon: &SessionIcon, policy: TexturePolicy) -> bool {
    policy.source_bytes(icon.width, icon.height, false) == Some(icon.rgba8.len())
        && icon
            .width
            .checked_add(GUTTER * 2)
            .zip(icon.height.checked_add(GUTTER * 2))
            .is_some_and(|(width, height)| policy.page_bytes(width, height).is_some())
}

#[cfg(test)]
thread_local! {
    /// Icon pages packed on this thread, so tests can tell the frame from a worker.
    static PACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
