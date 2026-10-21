//! Inventory block visuals share the terrain compiler without allocating palette states.

use super::*;

/// Compiles one default or authored inventory visual per block when the server declares any.
/// The temporary overlay is consumed by icon preparation and never enters the world registry.
pub(in crate::runtime::network) fn compile_block_item_overlay(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    keys: Option<&MaterialKeys>,
    max_layers: u32,
    base: Option<&assets::RuntimeAssets>,
    shared: Option<&SharedTextures>,
) -> Option<(CompiledBlockOverlay, CustomBlocks)> {
    if !blocks
        .blocks
        .iter()
        .any(|block| block.visual.item_visual.is_some())
    {
        return None;
    }
    let items = blocks
        .blocks
        .iter()
        .take_while(|block| !block.visual.materialization_disabled)
        .map(|block| {
            let mut item = block.clone();
            if let Some(visual) = &block.visual.item_visual {
                let definition = Arc::make_mut(&mut item.visual);
                definition.base = visual.clone();
                definition.permutations = Box::default();
            }
            let expressions = condition::BlockExpressions::new(&item);
            let values = block.state_values(0);
            let resolved = condition::state_visual(
                &item,
                &expressions,
                values.as_deref(),
                &mut OverlayGaps::default(),
            );
            let mut components = resolved.components;
            components.random_offset = None;
            for (bone, expression) in &mut components.bone_visibility {
                *expression = if resolved.hidden_bones.contains(bone) {
                    "0"
                } else {
                    "1"
                }
                .into();
            }
            item.state_count = 1;
            item.state_physics = Arc::default();
            item.visual = Arc::new(protocol::CustomBlockVisuals {
                base: components,
                ..Default::default()
            });
            item
        })
        .collect();
    let definitions = CustomBlocks {
        blocks: items,
        ..Default::default()
    };
    let compiled = compile_block_overlay_with_textures(
        view,
        &definitions,
        false,
        keys,
        max_layers,
        base,
        shared,
    )?;
    Some((compiled, definitions))
}
