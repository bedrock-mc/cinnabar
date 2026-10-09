//! Session armor texels shared by inventory previews and the HUD paper doll.

use super::*;

#[derive(Default)]
pub(in super::super) struct PackEquipment {
    pub(super) source: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    usable: bool,
    pub(in super::super) pages: Vec<UiTexturePage>,
    pub(in super::super) textures: BTreeMap<atlas::TextureKey, IconRef>,
}

impl PackEquipment {
    pub(in super::super) fn catalog(&self) -> Option<&assets::RuntimeEquipmentCatalog> {
        self.source.as_deref().filter(|_| self.usable)
    }
}

impl UiPresentationRuntime {
    /// Replaces session armor once per catalog change; removing the pack restores base art.
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
        let mut prepared = PackEquipment {
            source: catalog,
            ..Default::default()
        };
        if let Some(catalog) = prepared.source.as_deref() {
            if self.gui_models.enabled {
                let mut atlas = atlas::Atlas::new(0, MODEL_PAGES);
                let wanted = catalog
                    .bindings()
                    .iter()
                    .filter_map(|binding| {
                        matches!(binding.category, assets::EquipmentCategory::Armor { .. })
                            .then_some(binding.texture.identifier.as_ref())
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                let result = wanted
                    .into_iter()
                    .filter_map(|identifier| catalog.texture(identifier))
                    .try_for_each(|texture| {
                        atlas
                            .insert([texture.width, texture.height], &texture.rgba8)
                            .map(|_| ())
                    })
                    .and_then(|()| atlas.finish());
                match result {
                    Ok((pages, mut textures)) => {
                        let first = self.textures.dynamic_start() + MODEL_PAGE;
                        if self.gui_models.pages.len() + pages.len() > MODEL_PAGES
                            && self.gui_models.discard_optional_models(first)
                            && let Err(error) = self.install_gui_fire()
                        {
                            bevy::log::warn!(%error, "actor flames exceed the GUI texture budget");
                        }
                        if self.gui_models.pages.len() + pages.len() <= MODEL_PAGES {
                            let first = (first + self.gui_models.pages.len()) as u16;
                            for icon in textures.values_mut() {
                                icon.page += first;
                            }
                            prepared.pages = pages;
                            prepared.textures = textures;
                            prepared.usable = true;
                        } else {
                            bevy::log::warn!("pack armor exceeds the GUI texture budget");
                        }
                    }
                    Err(error) => {
                        bevy::log::warn!(%error, "pack armor exceeds the GUI texture budget")
                    }
                }
            } else {
                prepared.usable = true;
            }
        }
        self.gui_models.pack_equipment = prepared;
        self.rebuild_dynamic_textures();
    }
}

#[cfg(test)]
mod tests;
