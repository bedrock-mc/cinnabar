use super::*;

/// Creates a granted mask during init and traps when a later message arrives.
const MASK_GUEST: &str = r#"(component
        (import "cinnabar:world-draw/drawing@0.1.0" (instance $drawing
            (type $handle (record (field "owner" u64) (field "generation" u64) (field "slot" u64)))
            (export "handle" (type $h (eq $handle)))
            (type $code (enum "denied" "invalid" "budget-exceeded" "stale-handle" "unsupported" "in-use"))
            (export "error-code" (type $c (eq $code)))
            (type $error (record (field "code" $c) (field "detail" string)))
            (export "draw-error" (type $e (eq $error)))))
        (alias export $drawing "handle" (type $h))
        (alias export $drawing "draw-error" (type $e))
        (import "cinnabar:world-draw/masks@0.1.0" (instance $masks
            (export "handle" (type (eq $h)))
            (export "draw-error" (type (eq $e)))
            (type $depth (enum "test" "always"))
            (export "depth-policy" (type $d (eq $depth)))
            (export "create" (func (param "depth" $d) (result (result $h (error $e)))))))
        (core module $memory
            (memory (export "memory") 1)
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
        (core instance $mem (instantiate $memory))
        (alias core export $mem "memory" (core memory $memory-ref))
        (alias core export $mem "realloc" (core func $realloc))
        (alias export $masks "create" (func $create))
        (core func $create-low (canon lower (func $create) (memory $memory-ref) (realloc $realloc)))
        (core module $code
            (import "host" "create" (func $create (param i32 i32)))
            (import "host" "memory" (memory 1))
            (func (export "init")
                i32.const 0 i32.const 128 call $create
                i32.const 128 i32.load if unreachable end)
            (func (export "dispatch") (param i32 i32 i32 i32) unreachable))
        (core instance $host (export "create" (func $create-low)) (export "memory" (memory $memory-ref)))
        (core instance $run (instantiate $code (with "host" (instance $host))))
        (func (export "init") (canon lift (core func $run "init")))
        (func (export "dispatch") (param "channel" string) (param "record-json" (list u8))
            (canon lift (core func $run "dispatch") (memory $memory-ref) (realloc $realloc))))"#;

/// Instantiates the real mask guest with a bound native connection.
fn mask_host() -> BundleHost {
    BundleHost::launch(
        MASK_GUEST.as_bytes(),
        owner(),
        screen_capabilities(&[Permission::EntityHighlight]),
        1,
        Default::default(),
        drawing_context(1),
    )
    .unwrap()
}

#[test]
fn initialization_can_create_a_granted_entity_mask() {
    let host = mask_host();
    let output = host.store.data().presentation.presentation_output();
    assert_eq!(output.masks.len(), 1);
    assert!(output.masks[0].authorized());
    assert_eq!(output.masks[0].session.connection, 1);
}

#[test]
fn restart_binds_current_authority_before_reinitializing_masks() {
    let mut host = mask_host();
    let drawing = crate::presentation::Context {
        connection: 2,
        epoch: 2,
        dimension: 1,
        ..Default::default()
    };
    let message = Event::Message {
        channel: "demo.items".into(),
        record: b"[]".to_vec(),
    };
    assert!(host.dispatch(&message, 2, &drawing).is_err());
    assert!(host.active);
    let output = host.store.data().presentation.presentation_output();
    assert_eq!(output.masks.len(), 1);
    assert!(output.masks[0].authorized());
    assert_eq!(
        (
            output.masks[0].session.connection,
            output.masks[0].session.dimension
        ),
        (2, 1)
    );
}

#[test]
fn dispatch_refuses_unbound_stale_or_foreign_drawing_contexts() {
    let mut host = mask_host();
    let initial = host.store.data().presentation.presentation_output();
    for drawing in [
        crate::presentation::Context::default(),
        drawing_context(0),
        crate::presentation::Context {
            live: vec![mod_render::retained::EntityKey {
                session: 2,
                dimension: 0,
                runtime_id: 1,
                spawn_revision: 1,
            }],
            ..drawing_context(1)
        },
    ] {
        assert!(host.dispatch(&Event::Frame, 1, &drawing).is_err());
        assert_eq!(
            host.store.data().presentation.presentation_output().masks,
            initial.masks
        );
    }
    host.dispatch(&Event::Frame, 1, &drawing_context(1))
        .unwrap()
        .into_transaction();
}

/// Creates a canonical drawing instance through a real bundle host's shared imports.
fn placement(host: &mut BundleHost) -> crate::runtime::cinnabar::world_draw::drawing::Instance {
    use crate::runtime::cinnabar::world_draw::drawing as draw;
    use draw::Host as _;
    let state = &mut host.store.data_mut().presentation;
    state.grants.world_draw = true;
    let geometry = state
        .create_geometry(draw::GeometrySpec::Triangles(draw::Mesh {
            vertices: [(0., 0., 0.), (1., 0., 0.), (0., 1., 0.)]
                .map(|position| draw::Vertex {
                    position,
                    color: (1., 1., 1., 1.),
                    uv: (0., 0.),
                    data: (0., 0., 0., 0.),
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        }))
        .unwrap()
        .unwrap();
    state.commit_presentation();
    draw::Instance {
        geometry,
        material: None,
        params: Vec::new(),
        anchor: draw::Anchor::Camera,
        transform: draw::Transform {
            translation: (0., 0., 0.),
            rotation: (0., 0., 0., 1.),
            scale: (1., 1., 1.),
        },
        tint: (1., 1., 1., 1.),
        depth: draw::Depth::Test,
        blend: draw::Blend::Premultiplied,
        width: 1.,
        width_mode: draw::WidthMode::World,
        join: draw::Join::Bevel,
        cap: draw::Cap::Butt,
    }
}

#[test]
fn bundle_draw_limit_rejects_the_guest_call_before_it_can_publish() {
    use crate::runtime::cinnabar::world_draw::drawing as draw;
    use draw::Host as _;
    let mut host = mask_host();
    let instance = placement(&mut host);
    let state = &mut host.store.data_mut().presentation;
    state.begin_presentation();
    let error = state
        .replace_frame_group(
            "frame".into(),
            vec![instance.clone(); MAX_DRAWS as usize + 1],
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, draw::ErrorCode::BudgetExceeded);
    state
        .replace_frame_group("frame".into(), vec![instance.clone(); MAX_DRAWS as usize])
        .unwrap()
        .unwrap();
    let error = state.create(instance).unwrap().unwrap_err();
    assert_eq!(error.code, draw::ErrorCode::BudgetExceeded);
    state.commit_presentation();
    let mut receiver = crate::presentation::Receiver::default();
    receiver
        .apply(
            state.presentation_output(),
            mod_api::drawing::DrawingPermissions::ALL,
            &drawing_context(1),
            u64::MAX,
            |_| true,
        )
        .unwrap();
    assert_eq!(
        receiver.output().0.retained[0].instances.len(),
        MAX_DRAWS as usize
    );
}

#[test]
fn bundle_draw_limit_combines_immediate_and_retained_imports_in_either_order() {
    use crate::runtime::cinnabar::{extension::render, world_draw::drawing as draw};
    use {draw::Host as _, render::Host as _};
    for immediate_first in [false, true] {
        let mut host = mask_host();
        let instance = placement(&mut host);
        let state = &mut host.store.data_mut().presentation;
        state.begin_presentation();
        let lines = vec![render::Line {
            points: vec![
                render::Vector3 {
                    x: 0.,
                    y: 0.,
                    z: 0.,
                },
                render::Vector3 {
                    x: 1.,
                    y: 0.,
                    z: 0.,
                },
            ],
            width: 1.,
            color: render::Rgba {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 1.,
            },
            pattern: render::LinePattern::Solid,
        }];
        if immediate_first {
            state.draw_lines(lines.clone()).unwrap().unwrap();
        }
        let result = state
            .replace_frame_group("frame".into(), vec![instance; MAX_DRAWS as usize])
            .unwrap();
        if immediate_first {
            assert_eq!(result.unwrap_err().code, draw::ErrorCode::BudgetExceeded);
        } else {
            result.unwrap();
            assert!(state.draw_lines(lines).unwrap().is_err());
        }
    }
}
