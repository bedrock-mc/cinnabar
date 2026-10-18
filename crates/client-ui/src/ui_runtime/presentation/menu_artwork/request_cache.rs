use std::sync::Arc;

use launcher::dressing_room::{DressingRoomCape, DressingRoomSection, DressingRoomSkin};

use launcher::menu::{MenuScreen, MenuView};

pub(super) struct GalleryRequest {
    skins: Arc<[DressingRoomSkin]>,
    capes: Arc<[DressingRoomCape]>,
    catalog_generation: u64,
    equipped: Vec<cosmetics::Item>,
    cosmetic_indices: Vec<usize>,
    section: DressingRoomSection,
    selected: Option<usize>,
    selected_cape: Option<usize>,
    skin_indices: Vec<usize>,
    cape_indices: Vec<usize>,
    cape: Option<render_api::CapeImage>,
}

impl GalleryRequest {
    /// Retains changed gallery inputs once, including visible and equipped cosmetics.
    pub(super) fn capture(
        view: &MenuView,
        skins: &[usize],
        capes: &[usize],
        cosmetics: &[usize],
        equipped: &[cosmetics::Item],
    ) -> Option<Self> {
        (view.screen == MenuScreen::DressingRoom).then(|| Self {
            skins: view.dressing_room.skins.clone(),
            capes: view.dressing_room.capes.clone(),
            catalog_generation: cosmetics::Item::generation(),
            equipped: equipped.to_vec(),
            cosmetic_indices: cosmetics.to_vec(),
            section: view.dressing_room.section,
            selected: view.dressing_room.selected,
            selected_cape: view.dressing_room.selected_cape,
            skin_indices: skins.to_vec(),
            cape_indices: capes.to_vec(),
            cape: view.player_skin.as_ref().and_then(|skin| skin.cape.clone()),
        })
    }

    /// Compares an unchanged gallery without allocating or cloning the catalog.
    pub(super) fn matches(
        &self,
        view: &MenuView,
        skins: &[usize],
        capes: &[usize],
        cosmetics: &[usize],
        equipped: &[cosmetics::Item],
    ) -> bool {
        view.screen == MenuScreen::DressingRoom
            && Arc::ptr_eq(&self.skins, &view.dressing_room.skins)
            && Arc::ptr_eq(&self.capes, &view.dressing_room.capes)
            && self.catalog_generation == cosmetics::Item::generation()
            && self.equipped == equipped
            && self.cosmetic_indices == cosmetics
            && self.section == view.dressing_room.section
            && self.selected == view.dressing_room.selected
            && self.selected_cape == view.dressing_room.selected_cape
            && self.skin_indices.as_slice() == skins
            && self.cape_indices.as_slice() == capes
            && super::super::player_preview::cape::same(
                self.cape.as_ref(),
                view.player_skin
                    .as_ref()
                    .and_then(|skin| skin.cape.as_ref()),
            )
    }
}
