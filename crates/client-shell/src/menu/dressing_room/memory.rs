//! Catalog commands that do not need a native path or durable storage.

use super::*;
use crate::player_skin::catalog::memory as catalog;

/// Applies one bounded edit, preserving the same immutable-view publication contract as native.
pub(super) fn apply(view: &mut DressingRoomView, job: Job) -> Result<(), String> {
    match job {
        Job::Select(index) => {
            view.skins
                .get(index)
                .ok_or("This skin is no longer available.")?;
            view.selected = Some(index);
        }
        Job::SelectCape(index) => {
            if index.is_some_and(|index| view.capes.get(index).is_none()) {
                return Err("This cape is no longer available.".into());
            }
            view.selected_cape = index;
        }
        Job::SelectCosmetic(item) => crate::player_skin::catalog::toggle_cosmetic(view, item)?,
        Job::Model(model) => {
            let index = view.selected.ok_or("Select an imported skin first.")?;
            let geometry = catalog::geometry(view, model)?;
            let mut skins = view.skins.to_vec();
            let entry = skins
                .get_mut(index)
                .filter(|entry| entry.imported)
                .ok_or("Default skins keep their original model.")?;
            if entry.model == SkinModel::Custom || model == SkinModel::Custom {
                return Err("Custom models keep their authored geometry.".into());
            }
            entry.model = model;
            entry.skin.geometry = geometry;
            view.skins = skins.into();
        }
        Job::Rename(index, name) => {
            let mut skins = view.skins.to_vec();
            skins
                .get_mut(index)
                .filter(|entry| entry.imported)
                .ok_or("Only imported skins can be renamed.")?
                .name = catalog::valid_name(&name)?.to_owned();
            view.skins = skins.into();
        }
        Job::RenameCape(index, name) => {
            let mut capes = view.capes.to_vec();
            capes
                .get_mut(index)
                .filter(|entry| entry.imported)
                .ok_or("Only imported capes can be renamed.")?
                .name = catalog::valid_name(&name)?.to_owned();
            view.capes = capes.into();
        }
        Job::Delete(index) => {
            view.skins
                .get(index)
                .filter(|entry| entry.imported)
                .ok_or("Only imported skins can be deleted.")?;
            let mut skins = view.skins.to_vec();
            skins.remove(index);
            view.selected = match view.selected {
                Some(selected) if selected == index => {
                    Some(crate::player_skin::catalog::deletion_fallback(&skins)?)
                }
                selected => shifted(selected, index),
            };
            view.skins = skins.into();
        }
        Job::DeleteCape(index) => {
            view.capes
                .get(index)
                .filter(|entry| entry.imported)
                .ok_or("Only imported capes can be deleted.")?;
            let mut capes = view.capes.to_vec();
            capes.remove(index);
            view.selected_cape = match view.selected_cape {
                Some(selected) if selected == index => None,
                selected => shifted(selected, index),
            };
            view.capes = capes.into();
        }
        Job::Import(_) | Job::ImportCape(_) => {
            return Err("Choose files with the browser import control.".into());
        }
        #[cfg(target_arch = "wasm32")]
        Job::Pick(..) => unreachable!("picker results are handled by the browser executor"),
    }
    Ok(())
}

/// Keeps selections pointing to the same item after an earlier row is removed.
fn shifted(selected: Option<usize>, removed: usize) -> Option<usize> {
    selected.map(|index| index - usize::from(index > removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_import_identity_uses_the_image_key_even_with_a_session_prefix() {
        let fallback = crate::player_skin::LocalPlayerSkin::generated_default("identity fixture");
        let view = catalog::load(&fallback);
        let mut imported = view.selected_skin().unwrap().clone();
        imported.imported = true;
        imported.path = "retained.png".into();
        assert_eq!(
            crate::player_skin::selected_skin_id(&imported),
            protocol::custom_skin_id("retained")
        );
    }

    #[test]
    fn edits_reject_stale_indices_and_keep_fallback_identity_and_cosmetics() {
        let fallback = crate::player_skin::LocalPlayerSkin::generated_default("edits fixture");
        let mut view = catalog::load(&fallback);
        assert!(apply(&mut view, Job::Select(100)).is_err());
        apply(&mut view, Job::Select(0)).unwrap();
        assert_eq!(
            crate::player_skin::selected_skin_id(view.selected_skin().unwrap()),
            fallback.skin_id
        );
        assert!(apply(&mut view, Job::Model(SkinModel::Classic)).is_err());
        assert!(apply(&mut view, Job::Rename(0, "renamed".into())).is_err());
        assert!(apply(&mut view, Job::Delete(0)).is_err());
        apply(&mut view, Job::SelectCosmetic(None)).unwrap();
        apply(&mut view, Job::SelectCape(None)).unwrap();
        assert!(view.selected_cosmetics.is_empty());
        assert!(view.active_skin().unwrap().cape.is_none());
    }
}
