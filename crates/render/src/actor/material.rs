use assets::EntityRenderMaterialState;

#[cfg(test)]
use render_model::ActorMaterial;

/// Renderer-only routing for a color layer that must follow a sorted dissolve mask.
pub(crate) const LATE_DISSOLVE_COLOR: u32 = 1 << 31;

/// Translates admitted actor blending into the GPU pipeline state.
pub fn blend_state(
    state: EntityRenderMaterialState,
) -> Option<bevy::render::render_resource::BlendState> {
    use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};
    if !state.blend {
        return None;
    }
    let component = BlendComponent {
        src_factor: if state.additive && !state.additive_alpha {
            BlendFactor::One
        } else {
            BlendFactor::SrcAlpha
        },
        dst_factor: if state.additive {
            BlendFactor::One
        } else {
            BlendFactor::OneMinusSrcAlpha
        },
        operation: BlendOperation::Add,
    };
    Some(BlendState {
        color: component,
        alpha: component,
    })
}

pub(crate) fn state(word: u32) -> Option<EntityRenderMaterialState> {
    EntityRenderMaterialState::from_word(word & !LATE_DISSOLVE_COLOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_actor_states_roundtrip_without_changing_the_shader_kind() {
        for bits in 0..512 {
            let expected = EntityRenderMaterialState {
                alpha_test: bits & 1 != 0,
                cull: bits & 2 != 0,
                blend: bits & 4 != 0,
                depth_write: bits & 8 != 0,
                emissive: bits & 16 != 0,
                additive: bits & 32 != 0,
                additive_alpha: bits & 64 != 0,
                disable_overlay: bits & 128 != 0,
                depth_always: bits & 256 != 0,
            };
            let material = ActorMaterial {
                kind: assets::EntityRenderMaterial::Default,
                state: Some(expected),
                ..Default::default()
            };
            let word = material.gpu_word();
            assert_eq!(state(word), Some(expected));
            assert_eq!(
                word & EntityRenderMaterialState::KIND_MASK,
                material.kind as u32
            );
        }
        assert_eq!(
            ActorMaterial::default().gpu_word(),
            assets::EntityRenderMaterial::Default as u32
        );
        assert_eq!(state(ActorMaterial::default().gpu_word()), None);
    }
}
