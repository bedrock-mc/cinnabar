use super::{
    evaluation::Evaluator,
    pose::{LocalDelta, compose_pose, sample_clips},
    *,
};

/// One texture layer a rig draws this tick, from its render controllers in controller order.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderTextureLayer {
    pub material: assets::EntityRenderMaterial,
    /// Entity-catalog source index of the raster.
    pub source: u32,
    /// Additional samplers of the witnessed native three-texture material, not extra draws.
    pub multitexture: Option<[u32; 2]>,
    /// Multiplies the texture; white when the controller sets no colour.
    pub color: [f32; 4],
    /// Blended over the texture; alpha 0 when unset.
    pub overlay: [f32; 4],
    /// Bones this layer does not draw.
    pub hidden_bones: Arc<[u32]>,
    /// `uv_anim` `[offset u, offset v, scale u, scale v]`: `uv = offset + uv * scale`.
    pub uv_anim: [f32; 4],
    /// Catalog geometry this layer draws when its controller picks another than the rig's;
    /// `hidden_bones` and the poses then index that geometry's bones.
    pub geometry: Option<u32>,
    /// Empty for the tick-owned body; populated for alternate geometry or a frame-sampled body.
    pub previous_pose: Arc<[BoneTransform]>,
    pub pose: Arc<[BoneTransform]>,
    /// The controller draws unlit.
    pub ignore_lighting: bool,
}

/// Bones of a geometry a render controller draws beside the rig's own.
#[derive(Clone, Debug)]
pub(super) struct LayerSkeleton {
    bones: Vec<RuntimeBone>,
    names: Vec<Box<str>>,
}

pub(super) fn bind_attachable_roots(state: &mut ActorRigState, owner_names: &[Box<str>]) {
    attachable::bind_roots(&mut state.bones, &state.bone_names, owner_names);
    for skeleton in state.layer_skeletons.values_mut().flatten() {
        let skeleton = Arc::make_mut(skeleton);
        attachable::bind_roots(&mut skeleton.bones, &skeleton.names, owner_names);
    }
}

/// The rig whose render controllers are evaluated.
pub(super) struct RenderRig<'a> {
    pub binding: usize,
    /// Geometry the rig itself draws.
    pub geometry: u32,
    pub bone_names: &'a [Box<str>],
    pub skeletons: &'a BTreeMap<u32, Option<Arc<LayerSkeleton>>>,
}

/// Resolves, once per actor, every geometry the rig's controllers can choose.
pub(super) fn cache_layer_skeletons(assets: &RuntimeEntityAssets, state: &mut ActorRigState) {
    let render = assets.render_data();
    for layer in assets.render_layers(state.rig_binding) {
        let first = layer.first_geometry as usize;
        let Some(choices) = render
            .geometries
            .get(first..first + usize::from(layer.geometry_count))
        else {
            continue;
        };
        for choice in choices {
            state
                .layer_skeletons
                .entry(choice.geometry)
                .or_insert_with(|| {
                    let (bones, names) = resolve_bones(assets, choice.geometry as usize)?;
                    Some(Arc::new(LayerSkeleton { bones, names }))
                });
        }
    }
}

/// Poses each layer drawing its own geometry: the actor's clips, recompiled for that geometry,
/// bind to its bones by name as vanilla animates every controller's model.
pub(super) fn pose_layers(
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    skeletons: &BTreeMap<u32, Option<Arc<LayerSkeleton>>>,
    clips: &[super::tick::WeightedClip],
    layers: &mut [RenderTextureLayer],
    budget: &mut EvalBudget<'_>,
) {
    let assets = evaluator.assets;
    for layer in layers.iter_mut() {
        let Some(geometry) = layer.geometry else {
            continue;
        };
        let Some(Some(skeleton)) = skeletons.get(&geometry) else {
            continue;
        };
        let mapped: Vec<_> = clips
            .iter()
            .filter_map(|weighted| {
                let symbol = assets.animation_clips().get(weighted.clip)?.symbol;
                Some(super::tick::WeightedClip {
                    clip: assets.clip_for_geometry(symbol, geometry)? as usize,
                    ..*weighted
                })
            })
            .collect();
        // Keyframe scripts already ran for the rig; a scratch copy keeps them from running twice.
        let mut scratch = variables.clone();
        let local = sample_clips(evaluator, &mut scratch, &skeleton.bones, &mapped, budget)
            .unwrap_or_else(|_| vec![LocalDelta::default(); skeleton.bones.len()]);
        if let Some(pose) = compose_pose(&skeleton.bones, &local) {
            layer.pose = pose.into();
        }
    }
}

/// The pose of a layer that draws the rig's own geometry, shared by every such layer.
fn empty_pose() -> Arc<[BoneTransform]> {
    static EMPTY: std::sync::LazyLock<Arc<[BoneTransform]>> =
        std::sync::LazyLock::new(|| Arc::from([]));
    Arc::clone(&EMPTY)
}

/// Gives each layer the pose it drew last tick as its previous pose, so layers interpolate, and
/// keeps last tick's hidden-bone list when unchanged so its derived poses stay cached.
pub(super) fn carry_layer_poses(
    old: &[RenderTextureLayer],
    new: &mut [RenderTextureLayer],
    reset: bool,
) {
    for (index, layer) in new.iter_mut().enumerate() {
        if let Some(previous) = old.get(index)
            && previous.hidden_bones == layer.hidden_bones
        {
            layer.hidden_bones = Arc::clone(&previous.hidden_bones);
        }
        layer.previous_pose = match old.get(index) {
            Some(previous)
                if !reset
                    && previous.geometry == layer.geometry
                    && previous.pose.len() == layer.pose.len() =>
            {
                Arc::clone(&previous.pose)
            }
            _ => Arc::clone(&layer.pose),
        };
    }
}

/// The `uv_anim` value of a controller without one.
const IDENTITY_UV_ANIM: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// `pattern` is lowercase with an optional leading and/or trailing `*`; bone names match
/// ignoring ASCII case. Runs per rule, bone and actor every tick, so it never allocates.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    let (leading, rest) = match pattern.strip_prefix('*') {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let (trailing, core) = match rest.strip_suffix('*') {
        Some(core) => (true, core),
        None => (false, rest),
    };
    let (name, core) = (name.as_bytes(), core.as_bytes());
    let at = |start: usize| {
        name.get(start..start + core.len())
            .is_some_and(|window| window.eq_ignore_ascii_case(core))
    };
    match (leading, trailing) {
        (true, true) => (0..=name.len().saturating_sub(core.len())).any(at),
        (true, false) => name.len() >= core.len() && at(name.len() - core.len()),
        (false, true) => at(0),
        (false, false) => name.eq_ignore_ascii_case(core),
    }
}

fn color(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    components: Option<[u32; 4]>,
    default: [f32; 4],
    budget: &mut EvalBudget<'_>,
) -> Result<[f32; 4], EvalError> {
    let Some(components) = components else {
        return Ok(default);
    };
    let mut value = [0.0; 4];
    for ((slot, expression), this) in value.iter_mut().zip(components).zip(default) {
        let number = evaluator.number(expression as usize, variables, this, budget)?;
        *slot = if number.is_finite() { number } else { 0.0 };
    }
    Ok(value)
}

/// Evaluates the rig's render controllers against the actor's Molang state.
pub(super) fn evaluate_render(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    rig: RenderRig<'_>,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<RenderTextureLayer>, EvalError> {
    let assets = evaluator.assets;
    let render = assets.render_data();
    let mut output = Vec::new();
    let multitexture = assets::native_actor_uses_multitexture(assets, rig.binding);
    for layer in assets.render_layers(rig.binding) {
        budget.charge_work()?;
        if let Some(condition) = layer.condition
            && !evaluator
                .run(condition as usize, variables, 0.0, budget)?
                .truthy()
        {
            continue;
        }
        let choices = render
            .geometries
            .get(
                layer.first_geometry as usize
                    ..layer.first_geometry as usize + usize::from(layer.geometry_count),
            )
            .ok_or(EvalError::Invalid)?;
        let mut chosen = None;
        for choice in choices {
            budget.charge_work()?;
            let selected = match choice.condition {
                None => true,
                Some(condition) => evaluator
                    .run(condition as usize, variables, 0.0, budget)?
                    .truthy(),
            };
            if selected {
                chosen = Some(choice.geometry);
                break;
            }
        }
        let (geometry, bone_names) = match chosen.filter(|geometry| *geometry != rig.geometry) {
            None => (None, rig.bone_names),
            Some(geometry) => match rig.skeletons.get(&geometry) {
                Some(Some(skeleton)) => (Some(geometry), skeleton.names.as_slice()),
                // A geometry without a usable skeleton cannot be drawn.
                _ => continue,
            },
        };
        let rules = render
            .visibility
            .get(
                layer.first_visibility as usize
                    ..layer.first_visibility as usize + usize::from(layer.visibility_count),
            )
            .ok_or(EvalError::Invalid)?;
        let mut hidden = vec![false; bone_names.len()];
        for rule in rules {
            let visible = evaluator
                .run(rule.condition as usize, variables, 0.0, budget)?
                .truthy();
            for (index, name) in bone_names.iter().enumerate() {
                if pattern_matches(&rule.pattern, name) {
                    hidden[index] = !visible;
                }
            }
        }
        let hidden_bones: Arc<[u32]> = hidden
            .iter()
            .enumerate()
            .filter(|(_, hidden)| **hidden)
            .map(|(index, _)| index as u32)
            .collect();
        let tint = color(evaluator, variables, layer.color, [1.0; 4], budget)?;
        let overlay = color(evaluator, variables, layer.overlay_color, [0.0; 4], budget)?;
        let overlay = color(evaluator, variables, layer.hurt_color, overlay, budget)?;
        let uv_anim = color(
            evaluator,
            variables,
            layer.uv_anim,
            IDENTITY_UV_ANIM,
            budget,
        )?;
        let slots = render
            .slots
            .get(
                layer.first_slot as usize
                    ..layer.first_slot as usize + usize::from(layer.slot_count),
            )
            .ok_or(EvalError::Invalid)?;
        let mut selected_sources = Vec::with_capacity(slots.len());
        for slot in slots {
            let candidates = render
                .candidates
                .get(
                    slot.first_candidate as usize
                        ..slot.first_candidate as usize + usize::from(slot.candidate_count),
                )
                .ok_or(EvalError::Invalid)?;
            for candidate in candidates {
                budget.charge_work()?;
                let selected = match candidate.condition {
                    None => true,
                    Some(condition) => evaluator
                        .run(condition as usize, variables, 0.0, budget)?
                        .truthy(),
                };
                if selected {
                    selected_sources.push(candidate.source);
                    break;
                }
            }
        }
        if multitexture && selected_sources.len() != 3 {
            continue;
        }
        let grouped = if multitexture {
            Some([selected_sources[1], selected_sources[2]])
        } else {
            None
        };
        let count = if grouped.is_some() {
            1
        } else {
            selected_sources.len()
        };
        for &source in selected_sources.iter().take(count) {
            output.push(RenderTextureLayer {
                material: layer.material,
                source,
                multitexture: grouped,
                color: tint,
                overlay,
                hidden_bones: Arc::clone(&hidden_bones),
                uv_anim,
                geometry,
                previous_pose: empty_pose(),
                pose: empty_pose(),
                ignore_lighting: layer.ignore_lighting,
            });
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::pattern_matches;

    #[test]
    fn star_matches_everything_and_a_trailing_star_matches_a_prefix() {
        assert!(pattern_matches("*", "head"));
        assert!(pattern_matches("leg*", "legfront"));
        assert!(pattern_matches("head", "head"));
        assert!(!pattern_matches("head", "headwear"));
        assert!(!pattern_matches("leg*", "arm"));
        assert!(pattern_matches("*saddle*", "leftSaddleStrap"));
        assert!(pattern_matches("*ear", "MuleEar"));
        assert!(!pattern_matches("*saddle*", "body"));
        assert!(pattern_matches("leg*", "LegFront") && pattern_matches("head", "HEAD"));
        assert!(pattern_matches("*", "") && !pattern_matches("*ear", "ar"));
    }
}

#[cfg(test)]
#[path = "render/multitexture_tests.rs"]
mod multitexture_tests;
