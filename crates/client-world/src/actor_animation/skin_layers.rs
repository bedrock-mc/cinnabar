use std::borrow::Cow;

use protocol::{SkinAnimation, SkinAnimationKind, SkinGeometrySource};

use super::{pose::LocalDelta, *};

/// One animated persona geometry and texture beside the player's base model.
#[derive(Clone, Debug)]
pub struct SkinRenderLayer {
    pub image: SkinAnimation,
    pub geometry: Arc<assets::SkinGeometry>,
    /// Immutable worker-built mesh for this exact animated layer.
    pub mesh: Option<render_model::ActorRigGeometry>,
    pub previous: Arc<[BoneTransform]>,
    pub current: Arc<[BoneTransform]>,
    pub rest: Arc<[BoneTransform]>,
    pub hidden_bones: Arc<[u32]>,
    pub uv_anim: [f32; 4],
}

#[derive(Debug)]
pub(super) struct SkinLayerSkeleton {
    image: SkinAnimation,
    geometry: Arc<assets::SkinGeometry>,
    pub(super) mesh: Option<render_model::ActorRigGeometry>,
    pub(super) bones: Vec<RuntimeBone>,
    pub(super) names: Vec<Box<str>>,
    body_indices: Vec<Option<u32>>,
    pub(super) rest: Arc<[BoneTransform]>,
}

impl SkinLayerSkeleton {
    /// Whether `layer` was posed on this skeleton.
    pub(super) fn poses(&self, layer: &SkinRenderLayer) -> bool {
        self.image.kind == layer.image.kind && self.geometry.digest == layer.geometry.digest
    }
}

/// Resolves each animation image's own named geometry once per skin update.
pub(super) fn parse(
    source: &SkinGeometrySource,
    body_names: &[Box<str>],
) -> Vec<SkinLayerSkeleton> {
    source
        .animations
        .iter()
        .filter_map(|image| {
            let geometry = assets::parse_skin_geometry_layer(
                &source.resource_patch,
                &source.geometry_data,
                image.kind.geometry_key(),
            )
            .ok()??;
            let (bones, names) = skeleton(&geometry.bones)?;
            let rest = compose_pose(&bones, &[])?.into();
            Some(SkinLayerSkeleton {
                body_indices: names
                    .iter()
                    .map(|name| {
                        body_names
                            .iter()
                            .position(|body| body == name)
                            .map(|index| index as u32)
                    })
                    .collect(),
                image: image.clone(),
                mesh: render_model::skin_geometry(&geometry, render_model::DIAGNOSTIC_RIG_ID).ok(),
                geometry: Arc::new(geometry),
                bones,
                names,
                rest,
            })
        })
        .collect()
}

/// Looks up the compiled pack blink controller that vanilla adds for animated faces.
pub(super) fn blink_controller(
    assets: &RuntimeEntityAssets,
    state: &ActorRigState,
) -> Option<usize> {
    state
        .skin_skeleton()?
        .prepared
        .layers
        .iter()
        .any(|layer| layer.image.kind == SkinAnimationKind::Face)
        .then_some(())?;
    assets.controllers().iter().position(|controller| {
        assets
            .symbols()
            .get(controller.symbol as usize)
            .is_some_and(|symbol| {
                symbol.identifier.as_ref() == "controller.animation.persona.blink"
            })
    })
}

/// Supplies the frame counts and expression mode read by vanilla's persona controllers.
pub(super) fn seed(
    state: &ActorRigState,
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
) {
    let Some(skin) = state.skin_skeleton() else {
        return;
    };
    for layer in &skin.prepared.layers {
        let variable = match layer.image.kind {
            SkinAnimationKind::Face => "variable.animation_frames_face",
            SkinAnimationKind::Body32 => "variable.animation_frames_32x32",
            SkinAnimationKind::Body128 => "variable.animation_frames_128x128",
        };
        variables.set(
            evaluator.layout.slot(evaluator.assets, variable),
            layer.image.frames as f32,
        );
        if layer.image.kind == SkinAnimationKind::Face {
            variables.set(
                evaluator
                    .layout
                    .slot(evaluator.assets, "variable.use_blinking_animation"),
                f32::from(layer.image.blinking),
            );
        }
    }
}

/// Applies the pack's seven-frame-per-second scroll, or its evaluated blink state.
fn uv_offset(image: &SkinAnimation, life_seconds: f32, blinking: f32) -> f32 {
    if image.kind == SkinAnimationKind::Face && image.blinking {
        blinking * 0.5
    } else {
        (life_seconds * 7.0).floor().rem_euclid(image.frames as f32) / image.frames as f32
    }
}

/// Poses animation layers using the same named animation deltas as the base skin.
pub(super) fn evaluate(
    state: &ActorRigState,
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    local: &[LocalDelta],
    render: Option<&[RenderTextureLayer]>,
) -> Vec<SkinRenderLayer> {
    let Some(skin) = state.skin_skeleton() else {
        return Vec::new();
    };
    let blinking = variables
        .get(
            evaluator
                .layout
                .slot(evaluator.assets, "variable.is_blinking"),
        )
        .unwrap_or(0.0);
    skin.prepared
        .layers
        .iter()
        .filter_map(|layer| {
            let local = layer
                .names
                .iter()
                .map(|name| {
                    state
                        .bone_names
                        .iter()
                        .position(|driver| driver == name)
                        .and_then(|index| local.get(index).copied())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let pose: Arc<[BoneTransform]> = compose_pose(&layer.bones, &local)?.into();
            let (hidden_bones, uv_anim) = metadata(layer, evaluator, render, blinking);
            Some(SkinRenderLayer {
                image: layer.image.clone(),
                geometry: Arc::clone(&layer.geometry),
                mesh: layer.mesh.clone(),
                previous: Arc::clone(&pose),
                current: pose,
                rest: Arc::clone(&layer.rest),
                hidden_bones,
                uv_anim,
            })
        })
        .collect()
}

/// Derives atlas and visibility values from the immutable body-to-persona bone map.
fn metadata(
    layer: &SkinLayerSkeleton,
    evaluator: &Evaluator<'_>,
    render: Option<&[RenderTextureLayer]>,
    blinking: f32,
) -> (Arc<[u32]>, [f32; 4]) {
    let hidden = layer
        .names
        .iter()
        .zip(&layer.body_indices)
        .enumerate()
        .filter_map(|(index, (name, body_index))| {
            if !evaluator.context.is_local_first_person && matches!(name.as_ref(), "head" | "hat") {
                return None;
            }
            let body_index = (*body_index)?;
            render
                .and_then(|layers| layers.first())
                .is_some_and(|body| body.hidden_bones.binary_search(&body_index).is_ok())
                .then_some(index as u32)
        })
        .collect::<Arc<[_]>>();
    (
        hidden,
        [
            0.0,
            uv_offset(
                &layer.image,
                evaluator.life_tick as f32 * ANIMATION_TICK_SECONDS,
                blinking,
            ),
            1.0,
            1.0,
        ],
    )
}

/// Refreshes persona appearance without replacing either completed animation endpoint.
pub(super) fn refresh<'s>(
    state: &'s ActorRigState,
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    render: &[RenderTextureLayer],
    budget: &mut EvalBudget<'_>,
) -> Result<Cow<'s, [SkinRenderLayer]>, EvalError> {
    let Some(skin) = state.skin_skeleton() else {
        return Ok(Cow::Borrowed(&state.skin_layers));
    };
    let blinking = variables
        .get(
            evaluator
                .layout
                .slot(evaluator.assets, "variable.is_blinking"),
        )
        .unwrap_or(0.0);
    let mut refreshed: Option<Vec<SkinRenderLayer>> = None;
    for (index, old) in state.skin_layers.iter().enumerate() {
        let Some(layer) = skin.prepared.layers.iter().find(|layer| layer.poses(old)) else {
            continue;
        };
        for _ in &layer.names {
            budget.charge_work()?;
        }
        let (hidden_bones, uv_anim) = metadata(layer, evaluator, Some(render), blinking);
        if old.hidden_bones != hidden_bones || old.uv_anim != uv_anim {
            let target = &mut refreshed.get_or_insert_with(|| state.skin_layers.clone())[index];
            target.hidden_bones = hidden_bones;
            target.uv_anim = uv_anim;
        }
    }
    Ok(refreshed.map_or(Cow::Borrowed(&state.skin_layers), Cow::Owned))
}

/// Bounds the additional persona composition before sampling a body's frame pose.
pub(super) fn sample(
    state: &ActorRigState,
    evaluator: &Evaluator<'_>,
    variables: &MolangVariables,
    local: &[LocalDelta],
    render: Option<&[RenderTextureLayer]>,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<SkinRenderLayer>, EvalError> {
    let work = state.skin_skeleton().map_or(0, |skin| {
        skin.prepared
            .layers
            .iter()
            .map(|layer| layer.bones.len())
            .sum()
    });
    if work > budget.work_left {
        return Err(EvalError::ActorBudget);
    }
    budget.work_left -= work;
    Ok(evaluate(state, evaluator, variables, local, render))
}

/// Advances endpoints only on a tick, keeping view refreshes and replaced models separate.
pub(super) fn carry(
    previous: &[SkinRenderLayer],
    next: &mut [SkinRenderLayer],
    reset: bool,
    advance_history: bool,
) {
    if reset {
        return;
    }
    for layer in next {
        if let Some(old) = previous.iter().find(|old| {
            old.image.kind == layer.image.kind && old.geometry.digest == layer.geometry.digest
        }) {
            layer.previous = Arc::clone(if advance_history {
                &old.current
            } else {
                &old.previous
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_atlas_advances_at_pack_rate_and_uses_the_controller_blink_value() {
        let mut image = SkinAnimation {
            kind: SkinAnimationKind::Face,
            width: 32,
            height: 64,
            rgba8: Arc::from([]),
            frames: 2,
            blinking: false,
        };
        assert_eq!(uv_offset(&image, 1.0 / 7.0, 0.0), 0.5);
        assert_eq!(uv_offset(&image, 2.0 / 7.0, 0.0), 0.0);
        image.blinking = true;
        assert_eq!(uv_offset(&image, 0.0, 1.0), 0.5);
        assert_eq!(uv_offset(&image, 1.0 / 7.0, 0.0), 0.0);
    }
}
