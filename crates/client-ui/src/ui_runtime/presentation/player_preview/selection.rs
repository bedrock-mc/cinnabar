//! Worn preview selection uses the local actor's attachable query context.
use super::*;
use actor_animation::AttachablesRuntime;
use std::borrow::Cow;

struct SelectionAssets {
    source: Arc<assets::RuntimeEntityAssets>,
    runtime: AttachablesRuntime,
}

/// Retains the same bounded predicate caches as worn world rendering.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct EquipmentSelection {
    base: Option<SelectionAssets>,
    pack: Option<SelectionAssets>,
}

/// One admitted preview definition and its startup choice for an unavailable atlas region.
pub(in crate::ui_runtime::presentation) struct SelectedBinding<'a> {
    pub binding: &'a assets::EquipmentBinding,
    pub from_pack: bool,
    pub startup: Option<&'a assets::EquipmentBinding>,
}

impl EquipmentSelection {
    /// Selects session artwork before startup, sharing the caller's budget across both layers.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui_runtime::presentation) fn binding<'a>(
        &mut self,
        base: Option<&'a RuntimeEquipmentCatalog>,
        pack: Option<&'a RuntimeEquipmentCatalog>,
        identifier: &str,
        enchanted: bool,
        owner: Option<PreviewEquipmentOwner<'_>>,
        slot: Option<usize>,
        budget: &mut usize,
    ) -> Option<SelectedBinding<'a>> {
        let enchantment = [(
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(enchanted),
        )];
        let variables = if let Some(owner) = owner
            && !owner.input.owner_variables.is_empty()
        {
            Cow::Owned(
                owner
                    .input
                    .owner_variables
                    .iter()
                    .copied()
                    .chain(enchantment)
                    .collect::<Vec<_>>(),
            )
        } else {
            Cow::Borrowed(enchantment.as_slice())
        };
        let drawable = |binding: &assets::EquipmentBinding| {
            pack.and_then(|catalog| catalog.texture(&binding.texture.identifier))
                .or_else(|| base?.texture(&binding.texture.identifier))
                .is_some()
        };
        let mut select = |catalog: Option<&'a RuntimeEquipmentCatalog>,
                          layer: &mut Option<SelectionAssets>| {
            let catalog = catalog?;
            let index = if let (Some(owner), Some(layer)) = (owner, layer.as_mut()) {
                layer.runtime.select_equipment_binding(
                    catalog,
                    identifier,
                    owner.actor,
                    &owner.rig,
                    actor_animation::AttachableAnimationInput {
                        worn: slot.is_some(),
                        is_in_ui: true,
                        worn_slot: slot.unwrap_or_default() as u8,
                        owner_variables: &variables,
                        ..owner.input
                    },
                    budget,
                    drawable,
                )?
            } else {
                // Without an actor, only unrestricted ordinary definitions are safe to admit.
                return catalog
                    .bindings_for(identifier)
                    .iter()
                    .rev()
                    .find(|binding| {
                        unrestricted(binding, layer.as_ref().map(|layer| layer.source.as_ref()))
                            && drawable(binding)
                    });
            };
            catalog.bindings().get(index)
        };
        let selected = select(pack, &mut self.pack);
        let startup = select(base, &mut self.base);
        Some(SelectedBinding {
            binding: selected.or(startup)?,
            from_pack: selected.is_some(),
            startup,
        })
    }
    /// Returns the immutable geometry owner for the selected catalog layer.
    pub(in crate::ui_runtime::presentation) fn entities(
        &self,
        from_pack: bool,
    ) -> Option<Arc<assets::RuntimeEntityAssets>> {
        Some(Arc::clone(
            &if from_pack {
                self.pack.as_ref()?
            } else {
                self.base.as_ref()?
            }
            .source,
        ))
    }
}

/// Keeps predicate state until the immutable catalog changes, then drops its cached results.
fn sync(layer: &mut Option<SelectionAssets>, source: Option<Arc<assets::RuntimeEntityAssets>>) {
    if match (layer.as_ref(), source.as_ref()) {
        (Some(old), Some(source)) => Arc::ptr_eq(&old.source, source),
        (None, None) => true,
        _ => false,
    } {
        return;
    }
    *layer = source.map(|source| SelectionAssets {
        runtime: AttachablesRuntime::new(Arc::clone(&source)),
        source,
    });
}

/// The player and completed animation inputs that a worn preview observes.
#[derive(Clone, Copy, Debug)]
pub struct PreviewEquipmentOwner<'a> {
    pub actor: &'a dyn actor_animation::AnimatedActor,
    pub rig: actor_animation::ActorRigSnapshot<'a>,
    pub input: actor_animation::AttachableAnimationInput<'a>,
}

impl UiPresentationRuntime {
    /// Supplies the immutable startup and session catalogs that own item conditions.
    pub fn set_preview_equipment_assets(
        &mut self,
        base: Option<Arc<assets::RuntimeEntityAssets>>,
        pack: Option<Arc<assets::RuntimeEntityAssets>>,
    ) {
        sync(&mut self.preview_equipment_selection.base, base);
        sync(&mut self.preview_equipment_selection.pack, pack);
    }

    /// Dresses a preview using its player's inputs and each slot's stack enchantment flag.
    pub fn set_player_preview_gear_with_owner(
        &mut self,
        armor: [Option<(&str, Option<u32>, bool)>; 4],
        held: Option<(&str, u32)>,
        owner: PreviewEquipmentOwner<'_>,
    ) {
        self.set_preview_gear(armor, standalone_hands(held), Some(owner));
    }
}

/// Builds a main-hand preview without authoritative stack facts.
pub(super) fn standalone_hands(held: Option<(&str, u32)>) -> [Option<(PreviewHandItem, bool)>; 2] {
    [
        held.map(|(identifier, metadata)| {
            (
                PreviewHandItem {
                    grip: None,
                    glint: None,
                    identifier: Arc::from(identifier),
                    metadata,
                    use_animation: assets::item_use::ItemUseAnimation::None,
                    charged_projectile: assets::item_use::ItemProjectileRole::None,
                },
                false,
            )
        }),
        None,
    ]
}

/// Admits a context-free model only when its definition has no owner restriction.
pub(in crate::ui_runtime::presentation) fn unrestricted(
    binding: &assets::EquipmentBinding,
    entities: Option<&assets::RuntimeEntityAssets>,
) -> bool {
    binding.attachable.is_none()
        && entities.is_none_or(|entities| {
            entities
                .attachable_rig_binding(binding.attachable_identifier())
                .is_none_or(|rig| entities.rig_bindings()[rig].item_condition.is_none())
        })
}
