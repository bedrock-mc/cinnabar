use super::*;

const GRADE: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * param(0u); }";
const DEPTH: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(depth(uv)); }";
const LOOPED: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> { loop { } }";
const RESULT: u32 = 512;

/// Guest data at fixed offsets from 1024; each entry is (offset, length).
struct Data {
    bytes: Vec<u8>,
}

impl Data {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn push(&mut self, bytes: &[u8]) -> (u32, u32) {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = 1024 + self.bytes.len() as u32;
        self.bytes.extend_from_slice(bytes);
        (offset, bytes.len() as u32)
    }

    fn escaped(&self) -> String {
        self.bytes.iter().map(|b| format!("\\{b:02x}")).collect()
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// One 40-byte canonical-ABI decal: centre, radius, colour, progress, style.
fn decal(radius: f32) -> Vec<u8> {
    let mut bytes = floats(&[1.0, 64.0, 2.0, radius, 1.0, 0.0, 0.0, 0.5, 0.25]);
    bytes.extend([2, 0, 0, 0]);
    bytes
}

/// The result discriminant must match `error`, otherwise the guest traps.
fn check(error: bool) -> String {
    format!(
        "i32.const {RESULT} i32.load8_u i32.const {} i32.ne if unreachable end",
        u8::from(error)
    )
}

struct Calls {
    data: Data,
}

impl Calls {
    fn new() -> Self {
        Self { data: Data::new() }
    }

    fn register(
        &mut self,
        name: &str,
        order: i32,
        source: &str,
        depth: bool,
        error: bool,
    ) -> String {
        let (name_ptr, name_len) = self.data.push(name.as_bytes());
        let (src_ptr, src_len) = self.data.push(source.as_bytes());
        format!(
            "i32.const {name_ptr} i32.const {name_len} i32.const {order} i32.const {src_ptr} \
            i32.const {src_len} i32.const {} i32.const {RESULT} call $register {}",
            u8::from(depth),
            check(error)
        )
    }

    fn remove(&mut self, name: &str, error: bool) -> String {
        let (ptr, len) = self.data.push(name.as_bytes());
        format!(
            "i32.const {ptr} i32.const {len} i32.const {RESULT} call $remove {}",
            check(error)
        )
    }

    fn update(&mut self, name: &str, enabled: bool, params: &[f32], error: bool) -> String {
        let (name_ptr, name_len) = self.data.push(name.as_bytes());
        let (params_ptr, _) = self.data.push(&floats(params));
        format!(
            "i32.const {name_ptr} i32.const {name_len} i32.const {} i32.const {params_ptr} \
            i32.const {} i32.const {RESULT} call $update {}",
            u8::from(enabled),
            params.len(),
            check(error)
        )
    }

    fn draw(&mut self, decals: &[f32], error: bool) -> String {
        let bytes: Vec<u8> = decals.iter().flat_map(|&radius| decal(radius)).collect();
        let (ptr, _) = self.data.push(&bytes);
        format!(
            "i32.const {ptr} i32.const {} i32.const 0 i32.const 0 i32.const 0 i32.const 0 \
            i32.const 0 i32.const 0 i32.const {RESULT} call $draw {}",
            decals.len(),
            check(error)
        )
    }

    fn source(&self, init: &str, frame: &str) -> String {
        let package = include_str!("../../../mod-api/wit/extension.wit")
            .lines()
            .next()
            .unwrap()
            .trim_start_matches("package ")
            .trim_end_matches(';');
        let (name, version) = package.split_once('@').unwrap();
        include_str!("render.wat")
            .replace("$RENDER", &format!("{name}/render@{version}"))
            .replace("$DATA", &self.data.escaped())
            .replace("$INIT", init)
            .replace("$FRAME", frame)
    }
}

fn load(calls: &Calls, init: &str, frame: &str, grants: ModGrants) -> (tempfile::TempDir, ModHost) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("render.wat");
    std::fs::write(&path, calls.source(init, frame)).unwrap();
    let host = ModHost::load_with_grants(&path, grants).unwrap();
    (directory, host)
}

fn granted() -> ModGrants {
    ModGrants {
        render: true,
        ..Default::default()
    }
}

#[test]
fn render_is_denied_by_default_even_with_other_grants() {
    let mut calls = Calls::new();
    let init = calls.register("grade", 0, GRADE, false, true);
    let frame = calls.draw(&[2.0], true);
    let grants = ModGrants {
        environment: true,
        players: true,
        camera: true,
        item_use: true,
        controls: true,
        interaction: true,
        settings: false,
        render: false,
        render_depth: true,
        entities: true,
        commands: vec!["ability".into()],
        packet_delay: true,
        block_highlights: false,
        fullbright: false,
    };
    let (_dir, mut host) = load(&calls, &init, &frame, grants);
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(host.render().0, &mod_render::RenderOutput::default());
}

#[test]
fn passes_commit_sorted_and_params_update_per_frame() {
    let mut calls = Calls::new();
    let init = format!(
        "{} {}",
        calls.register("late", 5, GRADE, false, false),
        calls.register("early", -1, GRADE, false, false)
    );
    let frame = calls.update("late", false, &[0.5, 2.0], false);
    let (_dir, mut host) = load(&calls, &init, &frame, granted());
    let (output, before) = host.render();
    let names: Vec<_> = output.passes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["early", "late"]);
    assert!(
        output
            .passes
            .iter()
            .all(|p| p.enabled && p.params == [0.0; 16])
    );
    assert!(output.passes[0].shader.contains(GRADE));
    assert_ne!(output.passes[0].revision, output.passes[1].revision);
    host.frame(false).unwrap();
    let (output, after) = host.render();
    assert_ne!(before, after);
    assert!(!output.passes[1].enabled);
    assert_eq!(&output.passes[1].params[..3], &[0.5, 2.0, 0.0]);
    host.frame(false).unwrap();
    assert_eq!(
        host.render().1,
        after,
        "an identical update commits no change"
    );
}

#[test]
fn invalid_passes_reject_cleanly_without_committing() {
    let mut calls = Calls::new();
    let init = [
        calls.register("looped", 0, LOOPED, false, true),
        calls.register("Bad Name", 0, GRADE, false, true),
        calls.register("depth", 0, DEPTH, true, true),
        calls.update("missing", true, &[], true),
        calls.update("looped", true, &[f32::NAN], true),
        calls.remove("missing", true),
    ]
    .join(" ");
    let (_dir, host) = load(&calls, &init, "", granted());
    assert!(host.render().0.passes.is_empty());
    assert!(host.is_active());
    let mut calls = Calls::new();
    let init = calls.register("depth", 0, DEPTH, true, false);
    let grants = ModGrants {
        render_depth: true,
        ..granted()
    };
    let (_dir, host) = load(&calls, &init, "", grants);
    assert!(host.render().0.passes[0].depth);
}

#[test]
fn pass_count_is_bounded() {
    let mut calls = Calls::new();
    let mut init: Vec<_> = (0..mod_api::MAX_RENDER_PASSES)
        .map(|i| calls.register(&format!("p{i}"), 0, GRADE, false, false))
        .collect();
    init.push(calls.register("extra", 0, GRADE, false, true));
    init.push(calls.register("p0", 3, GRADE, false, false));
    let (_dir, host) = load(&calls, &init.join(" "), "", granted());
    assert_eq!(host.render().0.passes.len(), mod_api::MAX_RENDER_PASSES);
    assert_eq!(host.render().0.passes.last().unwrap().name, "p0");
}

#[test]
fn primitives_last_one_callback_and_respect_budgets() {
    let mut calls = Calls::new();
    let decal_frame = format!(
        "{} {}",
        calls.draw(&[2.0], false),
        calls.draw(&[mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS * 2.0], true)
    );
    let over = vec![1.0; mod_api::MAX_RENDER_DECALS + 1];
    let budget = calls.draw(&over, true);
    let frame = format!(
        "global.get $toggle i32.eqz if {decal_frame} {budget} end \
        global.get $toggle i32.eqz global.set $toggle"
    );
    let source = calls.source("", &frame).replace(
        "(data (i32.const 1024)",
        "(global $toggle (mut i32) (i32.const 0)) (data (i32.const 1024)",
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("render.wat");
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(&path, granted()).unwrap();
    host.frame(false).unwrap();
    let decals = &host.render().0.primitives.decals;
    assert_eq!(decals.len(), 1);
    assert_eq!(decals[0].center, [1.0, 64.0, 2.0]);
    assert_eq!(decals[0].style, mod_render::DecalStyle::Crater);
    host.frame(false).unwrap();
    assert!(host.render().0.primitives.is_empty());
}

#[test]
fn trap_clears_passes_and_primitives() {
    let mut calls = Calls::new();
    let init = calls.register("grade", 0, GRADE, false, false);
    let frame = format!("{} unreachable", calls.draw(&[2.0], false));
    let (_dir, mut host) = load(&calls, &init, &frame, granted());
    let before = host.render().1;
    assert!(host.frame(false).is_err());
    assert_eq!(host.render().0, &mod_render::RenderOutput::default());
    assert_ne!(host.render().1, before);
}

#[test]
fn render_call_budget_traps() {
    let mut calls = Calls::new();
    let init = calls.register("grade", 0, GRADE, false, false);
    let frame = calls.update("grade", true, &[1.0], false) + " ";
    let (_dir, mut host) = load(&calls, &init, &frame.repeat(33), granted());
    let error = host.frame(false).unwrap_err();
    assert!(format!("{error:#}").contains("render import budget exhausted"));
    assert!(host.render().0.passes.is_empty());
}

#[test]
fn reload_replaces_passes_with_the_new_instance() {
    let mut calls = Calls::new();
    let init = calls.register("old", 0, GRADE, false, false);
    let (directory, mut host) = load(&calls, &init, "", granted());
    let old = host.render().0.passes[0].revision;
    let mut calls = Calls::new();
    let init = calls.register("new", 0, GRADE, false, false);
    std::fs::write(directory.path().join("render.wat"), calls.source(&init, "")).unwrap();
    assert!(host.reload_if_changed().unwrap());
    let passes = &host.render().0.passes;
    assert_eq!(passes.len(), 1);
    assert_eq!(passes[0].name, "new");
    assert!(passes[0].revision > old);
}

#[test]
fn frames_may_compile_one_shader_each() {
    let mut calls = Calls::new();
    let other = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * 0.5; }";
    let frame = format!(
        "{} {} {}",
        calls.register("a", 0, GRADE, false, false),
        calls.register("b", 0, other, false, true),
        calls.register("a", 1, GRADE, false, false)
    );
    let (_dir, mut host) = load(&calls, "", &frame, granted());
    host.frame(false).unwrap();
    let passes = &host.render().0.passes;
    assert_eq!(passes.len(), 1);
    assert_eq!(
        passes[0].order, 1,
        "re-registering unchanged source compiles nothing"
    );
}

#[test]
fn identical_primitives_keep_their_identity_and_generation() {
    let mut calls = Calls::new();
    let frame = calls.draw(&[2.0], false);
    let (_dir, mut host) = load(&calls, "", &frame, granted());
    host.frame(false).unwrap();
    let (output, generation) = host.render();
    let first = std::sync::Arc::clone(&output.primitives);
    host.frame(false).unwrap();
    let (output, next) = host.render();
    assert_eq!(
        next, generation,
        "an unchanged frame publishes no new generation"
    );
    assert!(std::sync::Arc::ptr_eq(&first, &output.primitives));
}
