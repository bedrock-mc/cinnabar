use super::*;

#[test]
fn epoch_traps_quarantine_callbacks_before_output_is_committed() {
    let (engine, _ticker) = crate::execution::engine().unwrap();
    let mut instance = Instance::new(
        &engine,
        br#"(component
            (core module $code
                (memory 1)
                (func (export "init"))
                (func (export "frame") (loop $work
                    i32.const 0 i32.const 0 i32.const 65536 memory.fill br $work)))
            (core instance $run (instantiate $code))
            (func (export "init") (canon lift (core func $run "init")))
            (func (export "frame") (canon lift (core func $run "frame"))))"#,
        ModGrants::default(),
        String::new(),
        Declared::default(),
        None,
    )
    .unwrap();
    let result = instance.run(u64::MAX, commit, |exports, store| {
        // Explicitly expire this newly armed callback without waiting on wall time.
        store.epoch_deadline_callback(|_| Err(wasmtime::Trap::Interrupt.into()));
        store.set_epoch_deadline(0);
        exports.frame(store)
    });
    assert!(result.is_err());
    assert!(!instance.is_active());
    assert!(instance.label().is_none());
}
