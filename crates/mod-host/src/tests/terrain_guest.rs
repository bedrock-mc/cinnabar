//! Guest component that polls one terrain region per frame through the real `extension` world.

const SESSION_WIT: &str = include_str!("../../../experience-sdk/wit/session/session.wit");
const CAPABILITIES_WIT: &str =
    include_str!("../../../experience-sdk/wit/client/deps/server-experience/capabilities.wit");
const EXTENSION_WIT: &str = include_str!("../../../mod-api/wit/extension.wit");
/// Canonical-ABI return area: result tag at 0, read-result or error tag at 8, stamp at 16.
const RETURN_AREA: u32 = 0;
/// Whether a stamp is remembered, and the remembered stamp's fields.
const HAVE_STAMP: u32 = 504;
const STAMP: u32 = 512;
const HEAP: u32 = 1024;
/// `read-result` case tags.
pub const PENDING: u8 = 0;
pub const UNCHANGED: u8 = 1;
pub const CHANGED: u8 = 2;

/// Returns a component whose frame reads `[min, max)` with `geometry` flag bits, runs `check`,
/// then, when `remember`, passes the last delivered stamp as `after` on later frames.
pub fn terrain_guest(
    min: [i32; 3],
    max: [i32; 3],
    geometry: u8,
    remember: bool,
    check: &str,
) -> Vec<u8> {
    let package = EXTENSION_WIT
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("package "))
        .and_then(|line| line.strip_suffix(';'))
        .expect("extension WIT starts with its package");
    let (name, version) = package.split_once('@').expect("versioned package");
    let bounds = min
        .iter()
        .chain(&max)
        .map(|value| format!("i32.const {value}"))
        .collect::<Vec<_>>()
        .join(" ");
    let remember = if remember {
        format!(
            "i32.const {RETURN_AREA} i32.load8_u i32.eqz if
               i32.const {} i32.load8_u if
                 i32.const {STAMP} i32.const {} i64.load i64.store
                 i32.const {} i32.const {} i32.load i32.store
                 i32.const {} i32.const {} i64.load i64.store
                 i32.const {} i32.const {} i64.load i64.store
                 i32.const {HAVE_STAMP} i32.const 1 i32.store8
               end
             end",
            RETURN_AREA + 8,
            RETURN_AREA + 16,
            STAMP + 8,
            RETURN_AREA + 24,
            STAMP + 16,
            RETURN_AREA + 32,
            STAMP + 24,
            RETURN_AREA + 40,
        )
    } else {
        String::new()
    };
    let module = format!(
        r#"(module
  (import "{name}/terrain@{version}" "read-region"
    (func $read (param i32 i32 i32 i32 i32 i32 i32 i32 i64 i32 i64 i64 i32)))
  (memory (export "memory") 64)
  (global $next (mut i32) (i32.const {HEAP}))
  (global $frame (mut i32) (i32.const 0))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $at i32)
    global.get $next local.get 2 i32.add i32.const 1 i32.sub
    i32.const 0 local.get 2 i32.sub i32.and local.tee $at
    local.get 3 i32.add global.set $next local.get $at)
  (func (export "init"))
  (func (export "frame")
    i32.const {HEAP} global.set $next
    {bounds} i32.const {geometry}
    i32.const {HAVE_STAMP} i32.load8_u
    i32.const {STAMP} i64.load i32.const {} i32.load
    i32.const {} i64.load i32.const {} i64.load
    i32.const {RETURN_AREA} call $read
    {check}
    {remember}))"#,
        STAMP + 8,
        STAMP + 16,
        STAMP + 24,
    );
    let mut module = wat::parse_str(module).expect("terrain guest WAT");
    let mut resolve = wit_parser::Resolve::default();
    resolve.push_str("session.wit", SESSION_WIT).unwrap();
    resolve
        .push_str("capabilities.wit", CAPABILITIES_WIT)
        .unwrap();
    resolve
        .push_str(
            "drawing.wit",
            include_str!("../../../mod-api/wit/deps/world-draw/drawing.wit"),
        )
        .unwrap();
    let extension = resolve.push_str("extension.wit", EXTENSION_WIT).unwrap();
    let world = resolve
        .select_world(&[extension], Some("extension"))
        .unwrap();
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .unwrap();
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}

/// Traps unless each frame's read succeeded with the next `read-result` case in `tags`.
pub fn expect_reads(tags: &[u8]) -> String {
    let mut check = format!("i32.const {RETURN_AREA} i32.load8_u if unreachable end");
    for (frame, tag) in tags.iter().enumerate() {
        check += &format!(
            " global.get $frame i32.const {frame} i32.eq if \
             i32.const {} i32.load8_u i32.const {tag} i32.ne if unreachable end end",
            RETURN_AREA + 8
        );
    }
    check + " global.get $frame i32.const 1 i32.add global.set $frame"
}

/// Traps unless the read failed with the `read-error` case `code`.
pub fn expect_error(code: u8) -> String {
    format!(
        "i32.const {RETURN_AREA} i32.load8_u i32.eqz if unreachable end \
         i32.const {} i32.load8_u i32.const {code} i32.ne if unreachable end",
        RETURN_AREA + 8
    )
}
