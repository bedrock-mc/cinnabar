//! Session armor texels shared by inventory previews and the HUD paper doll.
//!
//! Only the pack textures the dressed model wears take GUI model pages. A pack ships far more
//! attachable art than the bounded model atlas holds, so placing all of it would fail as a
//! whole; placing the worn few, each on its own terms, keeps every other texture usable.

use super::*;

#[derive(Default)]
pub(in super::super) struct PackEquipment {
    pub(in super::super) source: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    /// The pack texture each armor slot wears, helmet to boots.
    worn: [Option<Box<str>>; 4],
    pub(in super::super) pages: Vec<UiTexturePage>,
    /// GUI regions of the worn pack textures, by texture identifier.
    pub(super) regions: BTreeMap<Box<str>, IconRef>,
}

impl PackEquipment {
    /// The GUI region of the pack texture armor slot `slot` wears, once placed.
    pub(in super::super) fn region(&self, slot: usize) -> Option<IconRef> {
        self.regions.get(self.worn.get(slot)?.as_deref()?).copied()
    }

    /// Distinct worn texture identifiers.
    fn wanted(&self) -> BTreeSet<&str> {
        self.worn.iter().flatten().map(AsRef::as_ref).collect()
    }
}

impl UiPresentationRuntime {
    /// Replaces the session's pack armor catalog; removing the pack restores base art.
    pub fn set_preview_pack_equipment(
        &mut self,
        catalog: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    ) {
        let previous = &self.gui_models.pack_equipment.source;
        if match (previous, &catalog) {
            (Some(previous), Some(next)) => Arc::ptr_eq(previous, next),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        let placed = !self.gui_models.pack_equipment.pages.is_empty();
        self.gui_models.pack_equipment = PackEquipment {
            source: catalog,
            ..Default::default()
        };
        if placed {
            self.rebuild_dynamic_textures();
        }
    }

    /// Records the pack texture each armor slot wears, placing them when the worn set changes;
    /// pieces that only trade slots keep their regions.
    pub(in super::super) fn wear_pack_armor(&mut self, worn: [Option<&str>; 4]) {
        let pack = &mut self.gui_models.pack_equipment;
        if pack.worn.each_ref().map(Option::as_deref) == worn {
            return;
        }
        let same_set = pack.wanted() == worn.iter().flatten().copied().collect::<BTreeSet<_>>();
        pack.worn = worn.map(|texture| texture.map(Box::from));
        if !same_set {
            self.place_pack_armor();
        }
    }

    /// Places the worn pack textures after the model pages that precede them, at full
    /// resolution where the bounded atlas allows. Call whenever those pages change.
    pub(super) fn place_pack_armor(&mut self) {
        let pack = &mut self.gui_models.pack_equipment;
        pack.pages.clear();
        pack.regions.clear();
        let catalog = pack.source.clone();
        let wanted = pack
            .wanted()
            .into_iter()
            .map(Box::from)
            .collect::<Vec<Box<str>>>();
        if let Some(catalog) = catalog.as_deref()
            && self.gui_models.enabled
            && !wanted.is_empty()
        {
            let mut textures = wanted
                .iter()
                .filter_map(|identifier| catalog.texture(identifier))
                .collect::<Vec<_>>();
            // Largest first: big art claims full resolution before small art fills the gaps.
            textures.sort_by_key(|texture| {
                std::cmp::Reverse(usize::from(texture.width) * usize::from(texture.height))
            });
            let first = self.textures.dynamic_start() + MODEL_PAGE;
            let available = |models: &GuiModels| MODEL_PAGES.saturating_sub(models.pages.len());
            let mut placed = place(&textures, available(&self.gui_models), false);
            if placed.regions.len() < textures.len()
                && self.gui_models.discard_optional_models(first)
            {
                if let Err(error) = self.install_gui_fire() {
                    bevy::log::warn!(%error, "actor flames exceed the GUI texture budget");
                }
                placed = place(&textures, available(&self.gui_models), false);
            }
            if placed.regions.len() < textures.len() {
                placed = place(&textures, available(&self.gui_models), true);
            }
            for texture in &textures {
                match placed.regions.get(texture.identifier.as_ref()) {
                    None => bevy::log::warn!(
                        texture = %texture.identifier,
                        "pack armor exceeds the GUI texture budget; base art stands in"
                    ),
                    Some((_, size)) if *size != [texture.width, texture.height] => {
                        bevy::log::info!(
                            texture = %texture.identifier,
                            source = ?[texture.width, texture.height],
                            placed = ?size,
                            "pack armor reduced to fit the GUI model atlas"
                        )
                    }
                    Some(_) => {}
                }
            }
            let start = (first + self.gui_models.pages.len()) as u16;
            let pack = &mut self.gui_models.pack_equipment;
            pack.pages = placed.pages;
            pack.regions = placed
                .regions
                .into_iter()
                .map(|(identifier, (mut icon, _))| {
                    icon.page += start;
                    (Box::from(identifier), icon)
                })
                .collect();
        }
        self.rebuild_dynamic_textures();
    }
}

/// Pages and page-relative regions (with the size stored) of one placement attempt.
struct Placement<'a> {
    pages: Vec<UiTexturePage>,
    regions: BTreeMap<&'a str, (IconRef, [u16; 2])>,
}

/// Places each texture independently within `limit` pages; one that cannot fit is left out
/// without disturbing the others. `shrink` reduces art past the budget instead.
fn place<'a>(
    textures: &[&'a assets::EquipmentTexture],
    limit: usize,
    shrink: bool,
) -> Placement<'a> {
    let mut atlas = atlas::Atlas::new(0, limit);
    let regions = textures
        .iter()
        .filter_map(|&texture| {
            let placed =
                atlas.insert_fitted([texture.width, texture.height], &texture.rgba8, shrink)?;
            Some((texture.identifier.as_ref(), placed))
        })
        .collect();
    match atlas.finish() {
        Ok((pages, _)) => Placement { pages, regions },
        Err(_) => Placement {
            pages: Vec::new(),
            regions: BTreeMap::new(),
        },
    }
}

#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod stack_tests;
#[cfg(test)]
mod tests;
