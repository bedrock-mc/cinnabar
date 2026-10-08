//! Sandboxed post-pass WGSL: a host prelude and entry points around one guest `effect`.
//!
//! The guest cannot declare resources or entry points and may not loop, so every fragment's
//! texture reads and IR expressions have a static worst case, which must stay within budget.

use mod_api::{
    MAX_SHADER_BYTES, MAX_SHADER_EXPRESSIONS, MAX_SHADER_TEXTURE_SAMPLES, MAX_SHADER_TYPE_BYTES,
    MAX_STATEMENT_TOKENS,
};
use naga::{Expression, Function, Handle, Module, Statement};

pub const VERTEX_ENTRY: &str = "mod_pass_vertex";
pub const FRAGMENT_ENTRY: &str = "mod_pass_fragment";
/// Bytes of the `ModFrame` uniform at binding 0.
pub const FRAME_UNIFORM_BYTES: usize = 2 * 64 + 3 * 16 + mod_api::MAX_PASS_PARAMS * 4;

const PRELUDE: &str = r#"
struct ModFrame {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    eye: vec4<f32>,
    resolution: vec4<f32>,
    time: vec4<f32>,
    params: array<vec4<f32>, 4>,
}
@group(0) @binding(0) var<uniform> frame: ModFrame;
@group(0) @binding(1) var scene_texture: texture_2d<f32>;
@group(0) @binding(2) var scene_sampler: sampler;
fn param(index: u32) -> f32 { return frame.params[min(index, 15u) / 4u][index % 4u]; }
fn seconds() -> f32 { return frame.time.x; }
fn scene(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scene_texture, scene_sampler, uv, 0.0).rgb;
}
fn luminance(color: vec3<f32>) -> f32 { return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)); }
fn world_to_uv(position: vec3<f32>) -> vec3<f32> {
    let clip = frame.clip_from_world * vec4<f32>(position, 1.0);
    if clip.w <= 0.0 { return vec3<f32>(-1.0, -1.0, -1.0); }
    let ndc = clip.xyz / clip.w;
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}
fn blur(uv: vec2<f32>, radius_px: f32) -> vec3<f32> {
    let o = frame.resolution.zw * radius_px;
    var c = scene(uv) * 0.25;
    c += (scene(uv + vec2<f32>(o.x, 0.0)) + scene(uv - vec2<f32>(o.x, 0.0))
        + scene(uv + vec2<f32>(0.0, o.y)) + scene(uv - vec2<f32>(0.0, o.y))) * 0.125;
    c += (scene(uv + o) + scene(uv - o) + scene(uv + vec2<f32>(o.x, -o.y))
        + scene(uv + vec2<f32>(-o.x, o.y))) * 0.0625;
    return c;
}
fn bright(uv: vec2<f32>, threshold: f32) -> vec3<f32> {
    return max(scene(uv) - vec3<f32>(threshold), vec3<f32>(0.0));
}
fn bloom(uv: vec2<f32>, radius_px: f32, threshold: f32) -> vec3<f32> {
    let o = frame.resolution.zw * radius_px;
    let near = bright(uv + o * vec2<f32>(1.0, 0.0), threshold)
        + bright(uv + o * vec2<f32>(0.5, 0.866), threshold)
        + bright(uv + o * vec2<f32>(-0.5, 0.866), threshold)
        + bright(uv + o * vec2<f32>(-1.0, 0.0), threshold)
        + bright(uv + o * vec2<f32>(-0.5, -0.866), threshold)
        + bright(uv + o * vec2<f32>(0.5, -0.866), threshold);
    let far = bright(uv + o * vec2<f32>(1.732, 1.0), threshold)
        + bright(uv + o * vec2<f32>(0.0, 2.0), threshold)
        + bright(uv + o * vec2<f32>(-1.732, 1.0), threshold)
        + bright(uv + o * vec2<f32>(-1.732, -1.0), threshold)
        + bright(uv + o * vec2<f32>(0.0, -2.0), threshold)
        + bright(uv + o * vec2<f32>(1.732, -1.0), threshold);
    return near * (1.0 / 9.0) + far * (1.0 / 18.0);
}
"#;

const DEPTH_PRELUDE: &str = r#"
@group(0) @binding(3) var depth_texture: texture_depth_2d;
fn depth(uv: vec2<f32>) -> f32 {
    let size = textureDimensions(depth_texture);
    let texel = vec2<u32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)) * vec2<f32>(size));
    return textureLoad(depth_texture, min(texel, max(size, vec2<u32>(1u)) - vec2<u32>(1u)), 0);
}
fn world_position(uv: vec2<f32>) -> vec3<f32> {
    let world = frame.world_from_clip * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth(uv), 1.0);
    return world.xyz / world.w;
}
"#;

const ENTRIES: &str = r#"
struct ModPassVertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, }
@vertex fn mod_pass_vertex(@builtin(vertex_index) index: u32) -> ModPassVertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return ModPassVertex(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
@fragment fn mod_pass_fragment(input: ModPassVertex) -> @location(0) vec4<f32> {
    return vec4<f32>(max(effect(input.uv), vec3<f32>(0.0)), 1.0);
}
"#;

/// Validation runs on its own thread so its recursion never depends on the caller's stack.
const VALIDATION_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Returns the complete host-composed module, or a guest-facing rejection.
pub fn compose(source: &str, depth: bool) -> Result<String, String> {
    if source.len() > MAX_SHADER_BYTES {
        return Err("shader exceeds byte limit".into());
    }
    check_statement_tokens(source)?;
    let source = source.to_owned();
    std::thread::Builder::new()
        .name("mod shader validation".into())
        .stack_size(VALIDATION_STACK_BYTES)
        .spawn(move || compose_checked(&source, depth))
        .map_err(|error| format!("shader validation unavailable: {error}"))?
        .join()
        .map_err(|_| "shader validation failed".to_owned())?
}

/// Counts identifier and number runs and other characters as tokens, ignoring comments.
fn check_statement_tokens(source: &str) -> Result<(), String> {
    let bytes = source.as_bytes();
    let (mut i, mut tokens, mut line) = (0, 0, 1);
    while i < bytes.len() {
        let byte = bytes[i];
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            let mut nesting = 0;
            while i < bytes.len() {
                if bytes[i..].starts_with(b"/*") {
                    nesting += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    nesting -= 1;
                    i += 2;
                    if nesting == 0 {
                        break;
                    }
                } else {
                    line += usize::from(bytes[i] == b'\n');
                    i += 1;
                }
            }
            continue;
        }
        i += 1;
        if byte.is_ascii_whitespace() {
            line += usize::from(byte == b'\n');
            continue;
        }
        if matches!(byte, b';' | b'{' | b'}') {
            tokens = 0;
            continue;
        }
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
        }
        tokens += 1;
        if tokens > MAX_STATEMENT_TOKENS {
            return Err(format!(
                "shader line {line}: statements may hold at most {MAX_STATEMENT_TOKENS} tokens"
            ));
        }
    }
    Ok(())
}

fn compose_checked(source: &str, depth: bool) -> Result<String, String> {
    let prelude = if depth {
        format!("{PRELUDE}{DEPTH_PRELUDE}")
    } else {
        PRELUDE.to_owned()
    };
    let composed = format!("{prelude}\n// guest\n{source}\n// host\n{ENTRIES}");
    let module = naga::front::wgsl::parse_str(&composed).map_err(|error| {
        let line = error.location(&composed).map(|location| {
            i64::from(location.line_number) - prelude.matches('\n').count() as i64 - 2
        });
        match line {
            Some(line) if line > 0 => format!("shader line {line}: {}", error.message()),
            _ => format!("shader rejected: {}", error.message()),
        }
    })?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| format!("shader invalid: {}", error.as_inner()))?;
    check_sandbox(&module, depth)?;
    Ok(composed)
}

fn check_type_sizes(module: &Module) -> Result<(), String> {
    let context = module.to_ctx();
    for (_, ty) in module.types.iter() {
        if ty.inner.size(context) > MAX_SHADER_TYPE_BYTES {
            return Err(format!(
                "shader types may hold at most {MAX_SHADER_TYPE_BYTES} bytes"
            ));
        }
    }
    Ok(())
}

fn check_sandbox(module: &Module, depth: bool) -> Result<(), String> {
    let mut globals: Vec<_> = module
        .global_variables
        .iter()
        .map(|(_, global)| global.name.as_deref().unwrap_or(""))
        .collect();
    globals.sort_unstable();
    let expected: &[&str] = if depth {
        &["depth_texture", "frame", "scene_sampler", "scene_texture"]
    } else {
        &["frame", "scene_sampler", "scene_texture"]
    };
    if globals != expected {
        return Err("shaders may not declare resources or module-scope variables".into());
    }
    check_type_sizes(module)?;
    if module.overrides.iter().next().is_some() {
        return Err("shaders may not declare overrides".into());
    }
    let mut entries: Vec<_> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    entries.sort_unstable();
    if entries != [FRAGMENT_ENTRY, VERTEX_ENTRY] {
        return Err("shaders may not declare entry points".into());
    }
    let mut costs = Vec::with_capacity(module.functions.len());
    for (_, function) in module.functions.iter() {
        let cost = cost(function, &costs)?;
        costs.push(cost);
    }
    let fragment = module
        .entry_points
        .iter()
        .find(|entry| entry.name == FRAGMENT_ENTRY)
        .expect("checked entry point");
    let total = cost(&fragment.function, &costs)?;
    if total.samples > MAX_SHADER_TEXTURE_SAMPLES {
        return Err(format!(
            "shader reads textures {} times per pixel; the limit is {MAX_SHADER_TEXTURE_SAMPLES}",
            total.samples
        ));
    }
    if total.expressions > MAX_SHADER_EXPRESSIONS {
        return Err(format!(
            "shader evaluates {} expressions per pixel; the limit is {MAX_SHADER_EXPRESSIONS}",
            total.expressions
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct Cost {
    samples: u32,
    expressions: u32,
}

/// Worst-case cost with every call site expanded; callees precede callers in the arena.
fn cost(function: &Function, callees: &[Cost]) -> Result<Cost, String> {
    let mut total = Cost {
        expressions: function.expressions.len() as u32,
        samples: function
            .expressions
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    Expression::ImageSample { .. } | Expression::ImageLoad { .. }
                )
            })
            .count() as u32,
    };
    walk(&function.body, callees, &mut total)?;
    Ok(total)
}

fn walk(block: &naga::Block, callees: &[Cost], total: &mut Cost) -> Result<(), String> {
    for statement in block.iter() {
        match statement {
            Statement::Loop { .. } => return Err("shaders may not loop".into()),
            Statement::Block(inner) => walk(inner, callees, total)?,
            Statement::If { accept, reject, .. } => {
                walk(accept, callees, total)?;
                walk(reject, callees, total)?;
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    walk(&case.body, callees, total)?;
                }
            }
            Statement::Call { function, .. } => {
                let callee = callee(*function, callees)?;
                total.samples = total.samples.saturating_add(callee.samples);
                total.expressions = total.expressions.saturating_add(callee.expressions);
            }
            Statement::ImageStore { .. }
            | Statement::ImageAtomic { .. }
            | Statement::Atomic { .. }
            | Statement::ControlBarrier(_)
            | Statement::MemoryBarrier(_)
            | Statement::WorkGroupUniformLoad { .. }
            | Statement::RayQuery { .. }
            | Statement::SubgroupBallot { .. }
            | Statement::SubgroupGather { .. }
            | Statement::SubgroupCollectiveOperation { .. } => {
                return Err("shaders may only compute a colour".into());
            }
            Statement::Emit(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Return { .. }
            | Statement::Kill
            | Statement::Store { .. } => {}
        }
    }
    Ok(())
}

fn callee(function: Handle<Function>, callees: &[Cost]) -> Result<Cost, String> {
    callees
        .get(function.index())
        .copied()
        .ok_or_else(|| "shader calls must not recurse".into())
}

#[cfg(test)]
mod tests;
