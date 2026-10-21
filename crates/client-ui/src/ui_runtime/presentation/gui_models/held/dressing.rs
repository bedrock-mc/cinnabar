//! Demand-bound authored hand models use the preview's selected definition and texture pages.
use super::*;
use crate::ui_runtime::presentation::{
    UiPresentationRuntime,
    player_preview::{PreviewEquipmentOwner, PreviewHandItem},
};

impl UiPresentationRuntime {
    /// Selects both hands with real stack inputs and retains at most one authored model per hand.
    pub(in crate::ui_runtime::presentation) fn prepare_preview_hands(
        &mut self,
        hands: &[Option<(PreviewHandItem, bool)>; 2],
        owner: Option<PreviewEquipmentOwner<'_>>,
        worn: [Option<&str>; 4],
    ) {
        let base = self.equipment_catalog.clone();
        let pack = self.gui_models.pack_equipment.source.clone();
        let selected: [_; 2] = std::array::from_fn(|index| {
            let (hand, enchanted) = hands[index].as_ref()?;
            let mut budget = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
            let owner = owner.map(|owner| PreviewEquipmentOwner {
                input: owner.input.for_hand(index == 1),
                ..owner
            });
            let selection = self.preview_equipment_selection.binding(
                base.as_deref(),
                pack.as_deref(),
                &hand.identifier,
                *enchanted,
                owner,
                None,
                &mut budget,
            )?;
            let binding = selection.binding;
            let mut entities = self
                .preview_equipment_selection
                .entities(selection.from_pack)?;
            if render_model::find_geometry_index(&entities, &binding.geometry.identifier).is_none()
            {
                entities = self.preview_equipment_selection.entities(false)?;
            }
            let artwork = pack
                .as_ref()
                .filter(|pack| pack.texture(&binding.texture.identifier).is_some())
                .or_else(|| {
                    base.as_ref()
                        .filter(|base| base.texture(&binding.texture.identifier).is_some())
                })?;
            let hand_pivots = self.gui_models.hand_pivots?;
            let model = if let Some(previous) = self.gui_models.selected_held[index]
                .as_ref()
                .filter(|previous| previous.matches(binding, &entities, artwork, hand_pivots))
            {
                previous.model.clone()
            } else {
                let texture = artwork.texture(&binding.texture.identifier)?;
                // Geometry must be drawable before its artwork can consume an atlas region.
                authored(
                    &entities,
                    artwork,
                    binding,
                    hand_pivots,
                    IconRef {
                        page: 0,
                        uv: [0, 0, texture.width, texture.height],
                        glint: false,
                    },
                )
            };
            Some((binding, entities, Arc::clone(artwork), model))
        });
        self.wear_pack_equipment(
            worn,
            selected.each_ref().map(|selected| {
                let (binding, _, artwork, model) = selected.as_ref()?;
                model.as_ref()?;
                pack.as_ref().filter(|pack| Arc::ptr_eq(pack, artwork))?;
                Some(binding.texture.identifier.as_ref())
            }),
        );
        for (index, selected) in selected.into_iter().enumerate() {
            let previous = self.gui_models.selected_held[index].take();
            self.gui_models.selected_held[index] = (|| {
                let (binding, entities, artwork, mut model) = selected?;
                let hand = &hands[index].as_ref()?.0;
                let hand_pivots = self.gui_models.hand_pivots?;
                let texture = artwork.texture(&binding.texture.identifier)?;
                let reusable = previous
                    .as_ref()
                    .filter(|previous| previous.matches(binding, &entities, &artwork, hand_pivots));
                let source = model.as_ref().and_then(|_| {
                    if pack
                        .as_ref()
                        .is_some_and(|pack| Arc::ptr_eq(pack, &artwork))
                    {
                        self.gui_models
                            .pack_equipment
                            .regions
                            .get(&binding.texture.identifier)
                            .copied()
                    } else if let Some(previous) = reusable {
                        previous
                            .model
                            .as_ref()
                            .filter(|_| previous.ready)
                            .map(|model| model.source)
                    } else {
                        self.gui_models
                            .textures
                            .get(&atlas::key([texture.width, texture.height], &texture.rgba8))
                            .copied()
                    }
                });
                let ready = source.is_some();
                if let (Some(source), Some(model)) = (source, model.as_mut()) {
                    model.source =
                        source.with_glint(entities.item_visuals().iter().any(|visual| {
                            visual.key.identifier.as_ref() == hand.identifier.as_ref()
                                && visual.key.metadata == hand.metadata
                                && visual.glint.enabled().unwrap_or(false)
                        }));
                }
                if let Some(mut previous) = previous
                    && previous.item.identifier.as_ref() == hand.identifier.as_ref()
                    && previous.item.metadata == hand.metadata
                    && previous.matches(binding, &entities, &artwork, hand_pivots)
                {
                    previous.model = model;
                    previous.ready = ready;
                    return Some(previous);
                }
                Some(SelectedModel {
                    item: ItemVisualKey {
                        identifier: hand.identifier.as_ref().into(),
                        metadata: hand.metadata,
                    },
                    artwork,
                    definition: binding.attachable_identifier().into(),
                    entities,
                    ready,
                    model,
                })
            })();
        }
    }
}
