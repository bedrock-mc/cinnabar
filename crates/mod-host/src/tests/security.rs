use super::*;

#[test]
fn guest_lists_share_a_copy_budget_before_capability_checks() {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let count = 32;
    let length = crate::execution::HOSTCALL_BYTES / count;
    assert!(4096 + length <= 65536);
    let mut frame = String::new();
    for index in 0..count {
        let descriptor = 2048 + index * 8;
        frame.push_str(&format!(
            "i32.const {descriptor} i32.const 4096 i32.store \
             i32.const {} i32.const {length} i32.store ",
            descriptor + 4
        ));
    }
    frame.push_str(&format!(
        "i32.const 1 i32.const 2048 i32.const {count} f32.const 64 \
         f32.const 1 f32.const 0.1 f32.const 0.7 f32.const 0.8 i32.const 512 call $set"
    ));
    let source = include_str!("block_highlights.wat")
        .replace("$RENDER", &format!("{name}/render@{version}"))
        .replace("$ERROR", "1")
        .replace("$INIT", "")
        .replace("$FRAME", &frame);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bounded.wat");
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load(&path).unwrap();
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("hostcalls"), "{error:#}");
    assert!(!host.is_active());
    assert!(host.block_highlights().is_none());
}

#[test]
fn guest_memory_ranges_are_checked_before_hud_output() {
    for frame in [
        "i32.const 65535 i32.const 5 i32.const 128 call $label",
        "i32.const -1 i32.const 2 i32.const 128 call $label",
        "i32.const 0 i32.const -1 i32.const 128 call $label",
    ] {
        let (_directory, mut host) = load_fixture(frame);
        assert!(host.frame(false).is_err());
        assert!(!host.is_active());
        assert!(host.label().is_none());
    }
}

#[test]
fn wasi_authority_is_unlinked() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unlinked.wat");
    for interface in [
        "wasi:filesystem/types@0.2.0",
        "wasi:cli/environment@0.2.0",
        "wasi:sockets/network@0.2.0",
        "wasi:clocks/wall-clock@0.2.0",
        "wasi:random/random@0.2.0",
    ] {
        let source = fixture("", "Hello").replacen(
            "(component",
            &format!("(component (import \"{interface}\" (instance (export \"probe\" (func))))"),
            1,
        );
        std::fs::write(&path, source).unwrap();
        // The packet-mod grants never link WASI into an ordinary mod.
        let trusted = ModGrants {
            packets: true,
            wasi: true,
            ..Default::default()
        };
        for grants in [ModGrants::default(), trusted] {
            let error = ModHost::load_with_grants(&path, grants)
                .err()
                .expect("authority must be unlinked");
            assert!(format!("{error:#}").contains(interface), "{error:#}");
        }
    }
}

#[test]
#[cfg(unix)]
fn settings_link_is_refused_before_guest_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = directory.path().join("mod.wat");
    let target = outside.path().join("preferences.json");
    std::fs::write(&path, fixture("", "Hello")).unwrap();
    std::fs::write(&target, "{\"value\":1}").unwrap();
    std::os::unix::fs::symlink(&target, path.with_extension("settings.json")).unwrap();
    let error = ModHost::load_with_grants(
        &path,
        ModGrants {
            settings: true,
            ..Default::default()
        },
    )
    .err()
    .expect("linked settings must fail admission");
    assert!(
        format!("{error:#}").contains("read mod settings"),
        "{error:#}"
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), "{\"value\":1}");
    assert!(
        ModHost::load(&path).is_ok(),
        "denied settings are never read"
    );
}
