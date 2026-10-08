use super::{
    evaluation::Evaluator,
    pose::{compose_pose, sample_clips},
    *,
};
use assets::entity_render_pattern_matches as pattern_matches;

/// One texture layer a rig draws this tick, from its render controllers in controller order.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderTextureLayer {
    pub material: assets::EntityRenderMaterial,
    pub material_state: Option<assets::EntityRenderMaterialState>,
    /// Entity-catalog source index of the raster.
    pub source: u32,
    /// Position among its controller layer's textures; a material samples later slots in the
    /// first slot's draw.
    pub texture_slot: u16,
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
    /// Multiplies light RGB after world-light admission; defaults to one.
    pub light_color_multiplier: f32,
    /// Frame-sampled authored uniform and X/Y/Z rig scales; absent for tick-owned placement.
    pub sampled_scale: Option<[f32; 4]>,
}

/// Bones of a geometry a render controller draws beside the rig's own.
#[derive(Clone, Debug)]
pub(super) struct LayerSkeleton {
    pub(super) bones: Vec<RuntimeBone>,
    pub(super) names: Vec<Box<str>>,
}

pub(super) fn bind_attachable_roots(state: &mut ActorRigState, owner_names: &[Box<str>]) {
    attachable::bind_roots(&mut state.bones, &state.bone_names, owner_names);
    for skeleton in state.layer_skeletons.values_mut().flatten() {
        let skeleton = Arc::make_mut(skeleton);
        attachable::bind_roots(&mut skeleton.bones, &skeleton.names, owner_names);
    }
}

/// The rig whose render controllers are evaluated.
#[derive(Clone, Copy)]
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
/// bind to its bones by name as vanilla animates every controller's model. Layers sharing a
/// geometry share one sampled pose.
pub(super) fn pose_layers(
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    skeletons: &BTreeMap<u32, Option<Arc<LayerSkeleton>>>,
    clips: &[super::tick::WeightedClip],
    layers: &mut [RenderTextureLayer],
    mut locals: Option<&mut BTreeMap<u32, Vec<pose::LocalDelta>>>,
    budget: &mut EvalBudget<'_>,
) {
    for index in 0..layers.len() {
        let Some(geometry) = layers[index].geometry else {
            continue;
        };
        let Some(Some(skeleton)) = skeletons.get(&geometry) else {
            continue;
        };
        let pose = match layers[..index]
            .iter()
            .find(|earlier| earlier.geometry == Some(geometry))
        {
            Some(earlier) => Some(Arc::clone(&earlier.pose)),
            None => sample_layer_local(evaluator, variables, skeletons, clips, geometry, budget)
                .ok()
                .and_then(|local| {
                    let pose = compose_pose(&skeleton.bones, &local).map(Arc::from);
                    if let Some(locals) = locals.as_deref_mut() {
                        locals.insert(geometry, local);
                    }
                    pose
                })
                .or_else(|| compose_pose(&skeleton.bones, &[]).map(Arc::from)),
        };
        if let Some(pose) = pose {
            layers[index].pose = pose;
        }
    }
}

/// Samples a selected geometry without mutating the rig variables.
pub(super) fn sample_layer_local(
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    skeletons: &BTreeMap<u32, Option<Arc<LayerSkeleton>>>,
    clips: &[super::tick::WeightedClip],
    geometry: u32,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<pose::LocalDelta>, EvalError> {
    let skeleton = skeletons
        .get(&geometry)
        .and_then(Option::as_ref)
        .ok_or(EvalError::Invalid)?;
    let assets = evaluator.assets;
    let mapped: Vec<_> = clips
        .iter()
        .filter_map(|weighted| clip_for_layer(assets, *weighted, geometry))
        .collect();
    // Keyframe scripts already ran for the rig; scratch variables keep their writes isolated.
    let mut scratch = variables.clone();
    let local = sample_clips(
        evaluator,
        &mut scratch,
        &skeleton.bones,
        &skeleton.names,
        &mapped,
        budget,
    )?;
    Ok(local)
}

pub(super) fn clip_for_layer(
    assets: &RuntimeEntityAssets,
    weighted: super::tick::WeightedClip,
    geometry: u32,
) -> Option<super::tick::WeightedClip> {
    let symbol = assets.animation_clips().get(weighted.clip)?.symbol;
    Some(super::tick::WeightedClip {
        clip: assets.clip_for_geometry(symbol, geometry)? as usize,
        ..weighted
    })
}

/// The pose of a layer that draws the rig's own geometry, shared by every such layer.
fn empty_pose() -> Arc<[BoneTransform]> {
    static EMPTY: std::sync::LazyLock<Arc<[BoneTransform]>> =
        std::sync::LazyLock::new(|| Arc::from([]));
    Arc::clone(&EMPTY)
}

/// Gives each layer the pose it drew last tick as its previous pose, so layers interpolate, and
/// retains prior endpoints on a view refresh and keeps unchanged hidden-bone lists cached.
pub(super) fn carry_layer_poses(
    old: &[RenderTextureLayer],
    new: &mut [RenderTextureLayer],
    reset: bool,
    advance_history: bool,
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
                Arc::clone(if advance_history {
                    &previous.pose
                } else {
                    &previous.previous_pose
                })
            }
            _ => Arc::clone(&layer.pose),
        };
    }
}

/// The `uv_anim` value of a controller without one.
const IDENTITY_UV_ANIM: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

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
        let light_color_multiplier = match layer.light_color_multiplier {
            None => 1.0,
            Some(expression) => {
                let value = evaluator.number(expression as usize, variables, 1.0, budget)?;
                if value.is_finite() { value } else { 1.0 }
            }
        };
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
        for (texture_slot, &source) in selected_sources.iter().take(count).enumerate() {
            output.push(RenderTextureLayer {
                material: layer.material,
                material_state: layer.material_state,
                source,
                texture_slot: texture_slot as u16,
                multitexture: grouped,
                color: tint,
                overlay,
                hidden_bones: Arc::clone(&hidden_bones),
                uv_anim,
                geometry,
                previous_pose: empty_pose(),
                pose: empty_pose(),
                ignore_lighting: layer.ignore_lighting,
                light_color_multiplier,
                sampled_scale: None,
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
