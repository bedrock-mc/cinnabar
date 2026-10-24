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
        .unwrap();
}
