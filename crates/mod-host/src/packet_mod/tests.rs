use super::*;

/// WASI preview 1 imports the WASI fixture calls, and the checks `start` makes before returning.
const WASI_IMPORTS: &str = r#"
  (import "wasi_snapshot_preview1" "fd_prestat_get" (func $prestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $env (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "args_sizes_get" (func $args (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "sock_accept" (func $accept (param i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "clock_time_get" (func $clock (param i32 i64 i32) (result i32)))
  (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
"#;
/// Refuses `start` unless descriptor 3 is no preopened directory (EBADF), the environment and
/// arguments are empty, and clocks, randomness and stdout work.
const WASI_CHECKS: &str = r#"
    (if (i32.ne (call $prestat (i32.const 3) (i32.const 64)) (i32.const 8)) (then (return (call $fail))))
    (if (call $env (i32.const 64) (i32.const 68)) (then (return (call $fail))))
    (if (i32.load (i32.const 64)) (then (return (call $fail))))
    (if (call $args (i32.const 64) (i32.const 68)) (then (return (call $fail))))
    (if (i32.load (i32.const 64)) (then (return (call $fail))))
    (if (call $clock (i32.const 1) (i64.const 1) (i32.const 64)) (then (return (call $fail))))
    (if (call $random (i32.const 64) (i32.const 8)) (then (return (call $fail))))
    (i32.store (i32.const 80) (i32.const 1048))
    (i32.store (i32.const 84) (i32.const 6))
    (if (call $write (i32.const 1) (i32.const 80) (i32.const 1) (i32.const 88)) (then (return (call $fail))))
"#;
/// Refuses `start` if accepting a connection on descriptor 3 succeeds.
const SOCKET_CHECK: &str = r#"
    (if (i32.eqz (call $accept (i32.const 3) (i32.const 0) (i32.const 64))) (then (return (call $fail))))
"#;

/// The fixture as a core module, with or without WASI imports and their checks.
fn core_module(wasi: bool, checks: &str) -> Vec<u8> {
    let imports = if wasi { WASI_IMPORTS } else { "" };
    let source = include_str!("tests/guest.wat")
        .replace("$IMPORTS", imports)
        .replace("$START", checks);
    wat::parse_str(source).unwrap()
}

/// Both accepted forms of the same guest: the core module and its packed component.
fn programs(wasi: bool) -> [(&'static str, Vec<u8>); 2] {
    let core = core_module(wasi, if wasi { WASI_CHECKS } else { "" });
    let component = packet_mod_component(&core).unwrap();
    [("core", core), ("component", component)]
}

fn grants(wasi: bool) -> ModGrants {
    ModGrants {
        packets: true,
        wasi,
        ..Default::default()
    }
}

fn identity() -> SessionIdentity {
    SessionIdentity {
        display_name: "Steve".into(),
        xuid: "2535".into(),
        uuid: "uuid".into(),
    }
}

fn instance(bytes: &[u8], wasi: bool) -> PacketMod {
    CompiledPacketMod::compile(bytes, &grants(wasi), None)
        .unwrap()
        .instantiate()
        .unwrap()
}

#[test]
fn start_receives_the_session_and_returns_packs() {
    for (form, bytes) in programs(false) {
        let mut guest = instance(&bytes, false);
        let packs = guest.start(&identity(), "{\"a\":1}", &[1, 2]).unwrap();
        assert_eq!(
            packs,
            [
                SessionPack {
                    archive: b"Steve".to_vec(),
                    above_server: true,
                },
                SessionPack {
                    archive: b"{\"a\":1}".to_vec(),
                    above_server: false,
                },
            ],
            "{form}"
        );
    }
}

#[test]
fn batches_pass_through_get_replaced_or_dropped() {
    for (form, bytes) in programs(false) {
        let mut guest = instance(&bytes, false);
        assert_eq!(
            guest.on_batch(Direction::FromClient, &[0, 9]).unwrap(),
            None,
            "{form}"
        );
        assert_eq!(
            guest.on_batch(Direction::FromClient, &[1]).unwrap(),
            Some(vec![0]),
            "{form}"
        );
        assert_eq!(
            guest.on_batch(Direction::FromServer, &[1]).unwrap(),
            Some(vec![1]),
            "{form}"
        );
        assert_eq!(
            guest.on_batch(Direction::FromServer, &[2]).unwrap(),
            Some(Vec::new()),
            "{form}"
        );
        assert_eq!(
            guest.tick().unwrap(),
            Outbound {
                to_client: b"c".to_vec(),
                to_server: b"s".to_vec(),
            },
            "{form}"
        );
        assert!(guest.is_active(), "{form}");
    }
}

#[test]
fn startup_uses_the_join_budget_and_play_retains_its_strict_budget() {
    for (form, bytes) in programs(false) {
        let mut guest = instance(&bytes, false);
        guest.call_deadline = Duration::ZERO;
        assert_eq!(
            guest.on_startup_batch(&[1]).unwrap(),
            Some(vec![1]),
            "{form}"
        );
        assert!(guest.is_active(), "{form}: startup should remain admitted");
        assert!(
            guest.on_batch(Direction::FromServer, &[1]).is_err(),
            "{form}"
        );
        assert!(
            !guest.is_active(),
            "{form}: play still enforces its deadline"
        );
    }
}

#[test]
fn failures_quarantine_into_passthrough() {
    for (form, bytes) in programs(false) {
        // Trap, guest refusal, memory past the limit, and a returned list out of bounds.
        for op in [3, 5, 6, 7] {
            let mut guest = instance(&bytes, false);
            assert!(
                guest.on_batch(Direction::FromServer, &[op]).is_err(),
                "{form} {op}"
            );
            assert!(!guest.is_active(), "{form} {op}");
            assert_eq!(
                guest.on_batch(Direction::FromServer, &[1]).unwrap(),
                None,
                "{form} {op}"
            );
            assert_eq!(guest.tick().unwrap().to_client, Vec::<u8>::new());
            assert!(guest.start(&identity(), "", &[]).unwrap().is_empty());
        }
    }
}

#[test]
fn a_missed_deadline_quarantines_the_instance() {
    for (form, bytes) in programs(false) {
        let mut guest = instance(&bytes, false);
        guest.call_deadline = Duration::from_millis(20);
        let error = guest.on_batch(Direction::FromClient, &[4]).unwrap_err();
        assert_eq!(
            error.root_cause().downcast_ref::<wasmtime::Trap>(),
            Some(&wasmtime::Trap::Interrupt),
            "{form}: {error:#}"
        );
        assert_eq!(guest.on_batch(Direction::FromClient, &[1]).unwrap(), None);
    }
}

#[test]
fn stop_ends_relaying() {
    for (form, bytes) in programs(false) {
        let mut guest = instance(&bytes, false);
        guest.stop();
        assert!(!guest.is_active(), "{form}");
        assert_eq!(guest.on_batch(Direction::FromClient, &[1]).unwrap(), None);
    }
}

#[test]
fn admission_needs_the_packets_grant_and_bounds_size() {
    let bytes = core_module(false, "");
    let denied = CompiledPacketMod::compile(&bytes, &ModGrants::default(), None);
    assert!(format!("{:#}", denied.err().unwrap()).contains("packets grant"));
    let oversized = vec![0; MAX_PACKET_MOD_BYTES + 1];
    let error = CompiledPacketMod::compile(&oversized, &grants(true), None).err();
    assert!(format!("{:#}", error.unwrap()).contains("exceeds"));
}

#[test]
fn wasi_stays_unlinked_without_its_grant() {
    for (form, bytes) in programs(true) {
        let error = CompiledPacketMod::compile(&bytes, &grants(false), None)
            .err()
            .expect("WASI must be unlinked");
        assert!(format!("{error:#}").contains("wasi"), "{form}: {error:#}");
    }
}

#[test]
fn granted_wasi_has_no_files_environment_or_sockets() {
    for (form, bytes) in programs(true) {
        let mut guest = instance(&bytes, true);
        let packs = guest.start(&identity(), "{}", &[]);
        assert_eq!(packs.unwrap().len(), 2, "{form}");
    }
}

#[test]
fn granted_wasi_refuses_sockets() {
    let core = core_module(true, SOCKET_CHECK);
    let mut guest = instance(&core, true);
    assert_eq!(guest.start(&identity(), "{}", &[]).unwrap().len(), 2);
    // The preview 1 adapter has no sockets at all: calling one traps.
    let component = packet_mod_component(&core).unwrap();
    let mut guest = instance(&component, true);
    assert!(guest.start(&identity(), "{}", &[]).is_err());
    assert!(!guest.is_active());
}

#[test]
fn compiled_code_is_cached_and_a_corrupt_artifact_recompiles() {
    let directory = tempfile::tempdir().unwrap();
    for (form, bytes) in programs(false) {
        for _ in 0..2 {
            let compiled =
                CompiledPacketMod::compile(&bytes, &grants(false), Some(directory.path())).unwrap();
            let mut guest = compiled.instantiate().unwrap();
            assert_eq!(
                guest.on_batch(Direction::FromServer, &[1]).unwrap(),
                Some(vec![1]),
                "{form}"
            );
        }
        let artifacts: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(artifacts.len(), 1, "{form}: older artifacts are pruned");
        std::fs::write(&artifacts[0], b"not compiled code").unwrap();
        let compiled =
            CompiledPacketMod::compile(&bytes, &grants(false), Some(directory.path())).unwrap();
        assert!(compiled.instantiate().is_ok(), "{form}");
    }
}
