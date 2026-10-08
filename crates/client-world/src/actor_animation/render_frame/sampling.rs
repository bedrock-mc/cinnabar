use super::*;

/// Lists the authored expressions that can affect an actor's rendered layers.
pub(super) fn render_expressions(assets: &RuntimeEntityAssets, binding: usize) -> Vec<u32> {
    let Some(rig) = assets.rig_bindings().get(binding) else {
        return Vec::new();
    };
    let mut expressions = Vec::new();
    expressions.extend(rig.pre_animation);
    expressions.extend(render_controller_expressions(assets, binding));
    expressions
}

/// Render-controller scripts run after body keyframes and before selected geometry sampling.
pub(super) fn render_controller_expressions(
    assets: &RuntimeEntityAssets,
    binding: usize,
) -> Vec<u32> {
    assets
        .render_layers(binding)
        .iter()
        .flat_map(|layer| render_layer_expressions(assets, layer).expressions)
        .collect()
}

/// Separate selection gates preserve dependencies of conditionally executed assignments.
pub(super) struct RenderExpressions {
    pub expressions: Vec<u32>,
    pub gates: Vec<u32>,
}

/// Layer conditions and choices control which following scripts execute.
pub(super) fn render_layer_expressions(
    assets: &RuntimeEntityAssets,
    layer: &assets::EntityRenderLayer,
) -> RenderExpressions {
    let data = assets.render_data();
    let mut expressions = Vec::new();
    let mut gates = Vec::new();

    expressions.extend(layer.condition);
    gates.extend(layer.condition);
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
        gates.extend(choices.iter().filter_map(|choice| choice.condition));
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
                gates.extend(choices.iter().filter_map(|choice| choice.condition));
            }
        }
    }
    RenderExpressions { expressions, gates }
}

/// Frame-time queries require fresh rendered-layer evaluation between simulation ticks.
pub(in crate::actor_animation) fn needs_frame_sampling(
    assets: &RuntimeEntityAssets,
    binding: usize,
) -> bool {
    uses_render_query(assets, binding, |name| {
        matches!(name, "query.frame_alpha" | "query.life_time")
    })
}

fn uses_render_query(
    assets: &RuntimeEntityAssets,
    binding: usize,
    query: impl Fn(&str) -> bool,
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
                    .is_some_and(|symbol| query(symbol.identifier.as_ref()))
            })
        })
}

/// Collects expressions reachable from a rig's render layers and animation topology.
pub(super) fn pose_expressions(
    assets: &RuntimeEntityAssets,
    rig: usize,
    geometry: usize,
    controllers: &[ControllerState],
) -> Vec<u32> {
    let mut expressions = render_expressions(assets, rig);
    if let Some(rig) = assets.rig_bindings().get(rig) {
        expressions.extend(rig.initialize);
        expressions.extend(rig.scale_expressions.into_iter().flatten());
    }
    let (selection, mut clips) = topology_expressions(assets, geometry, controllers);
    expressions.extend(selection);
    let geometries = assets
        .render_layers(rig)
        .iter()
        .flat_map(|layer| {
            let first = layer.first_geometry as usize;
            assets.render_data().geometries[first..first + usize::from(layer.geometry_count)]
                .iter()
                .map(|choice| choice.geometry)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mapped = clips
        .iter()
        .flat_map(|&clip| {
            let symbol = assets.animation_clips()[clip].symbol;
            geometries
                .iter()
                .filter_map(move |&geometry| assets.clip_for_geometry(symbol, geometry))
        })
        .map(|clip| clip as usize)
        .collect::<Vec<_>>();
    clips.extend(mapped);
    for clip in clips {
        let Some(clip) = assets.animation_clips().get(clip) else {
            continue;
        };
        expressions.extend(clip.anim_time_update);
        let first = clip.first_channel as usize;
        for channel in &assets.animation_channels()[first..first + clip.channel_count as usize] {
            let first = channel.first_keyframe as usize;
            for key in &assets.animation_keyframes()[first..first + channel.keyframe_count as usize]
            {
                expressions.extend(key.expressions.into_iter().flatten());
            }
        }
    }
    expressions.sort_unstable();
    expressions.dedup();
    expressions
}

/// Controller traversal and clocks precede bone channels on one variable stream.
pub(super) fn selection_expressions(
    assets: &RuntimeEntityAssets,
    geometry: usize,
    controllers: &[ControllerState],
) -> Vec<u32> {
    let (mut expressions, clips) = topology_expressions(assets, geometry, controllers);
    expressions.extend(
        clips
            .into_iter()
            .filter_map(|clip| assets.animation_clips()[clip].anim_time_update),
    );
    expressions
}

fn topology_expressions(
    assets: &RuntimeEntityAssets,
    geometry: usize,
    controllers: &[ControllerState],
) -> (Vec<u32>, std::collections::BTreeSet<usize>) {
    let mut clips = std::collections::BTreeSet::new();
    let mut expressions = Vec::new();
    if let Some(geometry) = assets.rig_geometries().get(geometry) {
        let first = geometry.first_animation as usize;
        for binding in
            &assets.rig_animations()[first..first + usize::from(geometry.animation_count)]
        {
            expressions.extend(binding.weight);
            clips.insert(binding.clip as usize);
        }
        let first = geometry.first_controller as usize;
        for binding in
            &assets.rig_controllers()[first..first + usize::from(geometry.controller_count)]
        {
            expressions.extend(binding.weight);
        }
    }
    for runtime in controllers {
        let Some(controller) = assets.controllers().get(runtime.controller) else {
            continue;
        };
        let first = controller.first_state as usize;
        for state in &assets.controller_states()[first..first + usize::from(controller.state_count)]
        {
            expressions.extend(state.on_entry);
            expressions.extend(state.on_exit);
            let first = state.first_transition as usize;
            expressions.extend(
                assets.controller_transitions()[first..first + usize::from(state.transition_count)]
                    .iter()
                    .map(|transition| transition.condition),
            );
            let first = state.first_animation as usize;
            for animation in
                &assets.controller_animations()[first..first + usize::from(state.animation_count)]
            {
                expressions.extend(animation.weight);
                if let assets::EntityControllerAnimationTarget::Clip(clip) = animation.target {
                    clips.insert(clip as usize);
                }
            }
        }
    }
    (expressions, clips)
}
