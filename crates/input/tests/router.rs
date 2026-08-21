use core::num::NonZeroU64;

use semantic_input::{
    Action, ActionBinding, AxisDirection, BindingError, ControlSettings, ControllerFrame,
    DeviceFrame, FrameError, InputChord, InputContext, KeyboardMouseFrame, MAX_CONTROLLERS,
    MAX_LOOK_DELTA_PER_FRAME, MAX_TOUCH_CONTROLS, ModifierChord, PhysicalControl, ReleaseReason,
    RouterError, SemanticInputRouter, TouchAxis, TouchControl, TouchControlKind,
    TouchControlLayout, TouchLayoutError,
};

fn empty_chord(control: PhysicalControl) -> InputChord {
    InputChord {
        control,
        modifiers: ModifierChord::default(),
    }
}

fn touch_button(hit_id: u16) -> TouchControl {
    TouchControl {
        hit_id,
        kind: TouchControlKind::Button,
    }
}

fn settings_with_deadzones(move_deadzone: f32, look_deadzone: f32) -> ControlSettings {
    ControlSettings::new(
        ControlSettings::default().bindings().to_vec(),
        1.0,
        1.0,
        1.0,
        false,
        false,
        move_deadzone,
        look_deadzone,
    )
    .unwrap()
}

fn assert_global_activity_contract(seed: DeviceFrame, source: fn(u64) -> DeviceFrame) {
    let mut router = SemanticInputRouter::default();
    router.route(seed).unwrap();
    router.finalize().unwrap();
    router.route(source(1)).unwrap();
    router.finalize().unwrap();
    for actual in [2, 10] {
        assert_eq!(
            router.route(source(actual)),
            Err(RouterError::NonMonotonicActivitySequence {
                previous: 10,
                actual,
            })
        );
    }
    router.route(source(11)).unwrap();
    assert_eq!(router.finalize().unwrap().frame_sequence, 3);
}

include!("router/core_and_sampling.rs");
include!("router/bindings_and_activity.rs");
include!("router/authority_and_neutrality.rs");
include!("router/controller_arbitration.rs");
include!("router/player_list.rs");
