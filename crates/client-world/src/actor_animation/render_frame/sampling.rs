use super::*;

/// Lists the authored expressions that can affect an actor's rendered layers.
pub(super) fn render_expressions(assets: &RuntimeEntityAssets, binding: usize) -> Vec<u32> {
    let Some(rig) = assets.rig_bindings().get(binding) else {
        return Vec::new();
    };
    let mut expressions = Vec::new();
    expressions.extend(rig.pre_animation);
    let data = assets.render_data();
    for layer in assets.render_layers(binding) {
        expressions.extend(layer.condition);
        expressions.extend(layer.light_color_multiplier);
        for channels in [
            layer.color,
            layer.overlay_color,
            layer.hurt_color,
            layer.on_fire_color,
            layer.uv_anim,
        ]
        .into_iter()
        .flatten()
        {
            expressions.extend(channels);
        }
        let first = layer.first_geometry as usize;
        if let Some(choices) = data
            .geometries
            .get(first..first + usize::from(layer.geometry_count))
        {
            expressions.extend(choices.iter().filter_map(|choice| choice.condition));
        }
        let first = layer.first_visibility as usize;
        if let Some(rules) = data
            .visibility
            .get(first..first + usize::from(layer.visibility_count))
        {
            expressions.extend(rules.iter().map(|rule| rule.condition));
        }
        let first = layer.first_slot as usize;
        if let Some(slots) = data.slots.get(first..first + usize::from(layer.slot_count)) {
            for slot in slots {
                let first = slot.first_candidate as usize;
                if let Some(choices) = data
                    .candidates
                    .get(first..first + usize::from(slot.candidate_count))
                {
                    expressions.extend(choices.iter().filter_map(|choice| choice.condition));
                }
            }
        }
    }
    expressions
}

/// Frame-time queries require fresh rendered-layer evaluation between simulation ticks.
pub(in crate::actor_animation) fn needs_frame_sampling(
    assets: &RuntimeEntityAssets,
    binding: usize,
) -> bool {
    render_expressions(assets, binding)
        .into_iter()
        .any(|expression| {
            let Some(expression) = assets.molang_expressions().get(expression as usize) else {
                return false;
            };
            let first = expression.first_op as usize;
            let Some(ops) = assets
                .molang_ops()
                .get(first..first + usize::from(expression.op_count))
            else {
                return false;
            };
            ops.iter().any(|op| {
                let symbol = match op {
                    MolangOp::LoadQuery(symbol) => *symbol,
                    MolangOp::CallQuery(call) => call.symbol,
                    _ => return false,
                };
                assets
                    .molang_symbols()
                    .get(symbol as usize)
                    .is_some_and(|symbol| {
                        matches!(
                            symbol.identifier.as_ref(),
                            "query.frame_alpha" | "query.life_time" | "query.swell_amount"
                        )
                    })
            })
        })
}
