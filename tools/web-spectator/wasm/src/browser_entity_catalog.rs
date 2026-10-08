//! World-actor selection from a verified artwork catalog that also contains attachables.

use assets::{ActorArtworkBinding, EntityAssetKind, EntityAssetSymbol};

pub(super) fn world_entity_bindings<'a>(
    symbols: &'a [EntityAssetSymbol],
    bindings: &'a [ActorArtworkBinding],
) -> impl Iterator<Item = (&'a EntityAssetSymbol, &'a ActorArtworkBinding)> {
    bindings.iter().filter_map(|binding| {
        let symbol = symbols.get(binding.entity_symbol as usize)?;
        (symbol.kind == EntityAssetKind::Entity).then_some((symbol, binding))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_artwork_catalog_does_not_admit_attachables_as_world_actors() {
        // Bow artwork uses an attachable texture mesh rather than an entity cube rig.
        let symbols = [
            (EntityAssetKind::Attachable, "minecraft:bow"),
            (EntityAssetKind::Entity, "minecraft:arrow"),
            (EntityAssetKind::Attachable, "minecraft:carved_pumpkin"),
        ]
        .map(|(kind, identifier)| EntityAssetSymbol {
            kind,
            identifier: identifier.into(),
            source_index: 0,
            dependencies: Box::new([]),
        });
        let bindings = [0, 1, 2].map(|entity_symbol| ActorArtworkBinding {
            rig: entity_symbol,
            geometry_candidate: entity_symbol + 10,
            entity_symbol,
            geometry: entity_symbol,
            render_controller: 0,
            texture: 0,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        });
        let selected = world_entity_bindings(&symbols, &bindings)
            .map(|(symbol, binding)| (symbol.identifier.as_ref(), binding.geometry_candidate))
            .collect::<Vec<_>>();
        assert_eq!(selected, [("minecraft:arrow", 11)]);
    }
}
