//! Compose production WESL modules for shader validation and native pixel fixtures.
#![allow(
    dead_code,
    reason = "shared helpers serve different shader test targets"
)]
use crate::material_shader;

const VIEW: &str = "struct View { clip_from_world: mat4x4<f32>, unjittered_clip_from_world: mat4x4<f32>, view_from_world: mat4x4<f32>, world_from_view: mat4x4<f32>, clip_from_view: mat4x4<f32>, view_from_clip: mat4x4<f32>, world_position: vec3<f32>, exposure: f32, viewport: vec4<f32>, }";
const FULLSCREEN: &str = "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, }";
const FULLSCREEN_VERTEX: &str = "@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> FullscreenVertexOutput { var out: FullscreenVertexOutput; out.uv = vec2(f32((index << 1u) & 2u), f32(index & 2u)); out.position = vec4(out.uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0); return out; }";

/// Reads the alpha cutoff guarding a fragment discard, including named WGSL constants.
pub fn alpha_discard_threshold(source: &str, function: &str) -> Option<f32> {
    let module = naga::front::wgsl::parse_str(&standalone(source, &[])).expect("shader parses");
    // An entry point, or the shading function an entry point delegates to.
    let function = module
        .entry_points
        .iter()
        .find(|point| point.name == function)
        .map(|point| &point.function)
        .or_else(|| {
            module
                .functions
                .iter()
                .map(|(_, candidate)| candidate)
                .find(|candidate| candidate.name.as_deref() == Some(function))
        })
        .expect("fragment function exists");
    discard_threshold(&module, function, &function.body)
}

/// Follows nested fragment guards to the comparison that actually discards its alpha.
fn discard_threshold(
    module: &naga::Module,
    function: &naga::Function,
    block: &naga::Block,
) -> Option<f32> {
    block.iter().find_map(|statement| match statement {
        naga::Statement::If {
            condition,
            accept,
            reject,
        } => {
            let threshold = accept
                .iter()
                .any(|statement| matches!(statement, naga::Statement::Kill))
                .then(|| alpha_comparison(module, function, *condition))
                .flatten();
            threshold
                .or_else(|| discard_threshold(module, function, accept))
                .or_else(|| discard_threshold(module, function, reject))
        }
        naga::Statement::Block(inner) => discard_threshold(module, function, inner),
        _ => None,
    })
}

/// Resolves the cutoff of an alpha-channel comparison inside a combined material guard.
fn alpha_comparison(
    module: &naga::Module,
    function: &naga::Function,
    condition: naga::Handle<naga::Expression>,
) -> Option<f32> {
    if let naga::Expression::Load { pointer } = function.expressions[condition] {
        return stored_alpha_comparison(module, function, &function.body, pointer, condition);
    }
    let naga::Expression::Binary { op, left, right } = function.expressions[condition] else {
        return None;
    };
    let alpha = match function.expressions[left] {
        naga::Expression::Load { pointer } => &function.expressions[pointer],
        _ => &function.expressions[left],
    };
    if op == naga::BinaryOperator::Less
        && matches!(alpha, naga::Expression::AccessIndex { index: 3, .. })
    {
        let expression = match function.expressions[right] {
            naga::Expression::Constant(constant) => {
                &module.global_expressions[module.constants[constant].init]
            }
            _ => &function.expressions[right],
        };
        return match expression {
            naga::Expression::Literal(naga::Literal::F32(value)) => Some(*value),
            _ => None,
        };
    }
    alpha_comparison(module, function, left).or_else(|| alpha_comparison(module, function, right))
}

/// Follows Naga's temporary stores for short-circuit guards before the discard reads them.
fn stored_alpha_comparison(
    module: &naga::Module,
    function: &naga::Function,
    block: &naga::Block,
    pointer: naga::Handle<naga::Expression>,
    before: naga::Handle<naga::Expression>,
) -> Option<f32> {
    block.iter().find_map(|statement| match statement {
        naga::Statement::Store {
            pointer: target,
            value,
        } if value.index() < before.index()
            && (*target == pointer
                || matches!(
                    (&function.expressions[*target], &function.expressions[pointer]),
                    (naga::Expression::LocalVariable(left), naga::Expression::LocalVariable(right))
                        if left == right
                )) =>
        {
            alpha_comparison(module, function, *value)
        }
        naga::Statement::If { accept, reject, .. } => {
            stored_alpha_comparison(module, function, accept, pointer, before)
                .or_else(|| stored_alpha_comparison(module, function, reject, pointer, before))
        }
        naga::Statement::Block(inner) => {
            stored_alpha_comparison(module, function, inner, pointer, before)
        }
        _ => None,
    })
}

/// Resolves conditional attributes while retaining the module's imports and declaration names.
pub fn preprocess(source: &str, definitions: &[&str]) -> String {
    let source = material_shader::source(source);
    let mut module: wesl::syntax::TranslationUnit = source.parse().expect("WESL parses");
    wesl::pass::condcomp(&mut module, &features(definitions)).expect("shader flags resolve");
    module.to_string()
}

/// Preserves helper names so native fixtures can call production shading functions directly.
pub fn standalone(source: &str, definitions: &[&str]) -> String {
    compile(source, definitions, false)
}

/// Uses Bevy's WESL composition and pruning options before validating the resulting WGSL.
pub fn composed(source: &str, definitions: &[&str]) -> String {
    let source = if source.contains("import bevy_core_pipeline::fullscreen_vertex_shader") {
        format!("{source}\n{FULLSCREEN_VERTEX}")
    } else {
        source.to_owned()
    };
    let source = compile(&source, definitions, true);
    let module = naga::front::wgsl::parse_str(&source).expect("composed WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("composed module validates");
    source
}

/// Enables precisely the requested pipeline flags; unspecified flags stay disabled.
fn features(definitions: &[&str]) -> wesl::Features {
    let mut features = wesl::Features::default();
    for definition in definitions {
        features.set(*definition, true);
    }
    features
}

/// Resolves generated constants and all production imports through the WESL compiler.
fn compile(source: &str, definitions: &[&str], prune: bool) -> String {
    let mut resolver = wesl::resolver::VirtualResolver::new();
    for (path, source) in [
        ("bevy_render::view", VIEW.to_owned()),
        (
            "bevy_render::globals",
            "struct Globals { time: f32, delta_time: f32, frame_count: u32 }".to_owned(),
        ),
        (
            "bevy_core_pipeline::fullscreen_vertex_shader::fullscreen",
            FULLSCREEN.to_owned(),
        ),
        (
            "render::material",
            material_shader::source(include_str!("../../../src/material.wesl")),
        ),
        (
            "render::lighting",
            material_shader::source(include_str!("../../../src/lighting.wesl")),
        ),
        (
            "render::biome_tint",
            material_shader::bind_biome_tables(&meshing::biome_lattice::shader_source(
                include_str!("../../../src/biome_tint.wesl"),
            )),
        ),
        (
            "render::world_projection",
            include_str!("../../../src/world_projection.wesl").to_owned(),
        ),
        (
            "render::chunk_bindings",
            material_shader::source(include_str!("../../../src/chunk_bindings.wesl")),
        ),
        (
            "render::enhanced::common",
            include_str!("../../../src/enhanced/common.wesl").to_owned(),
        ),
        (
            "render::enhanced::view",
            include_str!("../../../src/enhanced/view.wesl").to_owned(),
        ),
        (
            "render::enhanced::caster",
            include_str!("../../../src/enhanced/caster.wesl").to_owned(),
        ),
        (
            "render::liquid",
            material_shader::source(include_str!("../../../src/liquid.wesl")),
        ),
        (
            "render::model",
            material_shader::source(include_str!("../../../src/model.wesl")),
        ),
    ] {
        resolver.add_module(path.parse().expect("module path"), source.into());
    }
    let root: wesl::syntax::ModulePath = "fixture::shader".parse().expect("root module path");
    resolver.add_module(
        root.clone(),
        material_shader::source(&meshing::cloud_viewport::shader_source(source)).into(),
    );
    let options = wesl::CompileOptions {
        visibility: false,
        features: features(definitions),
        strip: prune,
        mangler: if prune {
            wesl::ManglerKind::Escape
        } else {
            wesl::ManglerKind::None
        },
        ..Default::default()
    };
    wesl::compile(&root, &options, &FixtureResolver(resolver))
        .unwrap_or_else(|error| panic!("{}", error.diagnostic().render_plain()))
        .to_string()
}

/// Resolves dependency packages with the same crate names used by Bevy's shader cache.
struct FixtureResolver(wesl::resolver::VirtualResolver<'static>);

impl wesl::Resolver for FixtureResolver {
    /// Reads a fixture module after removing the importing package's dependency prefix.
    fn resolve_source<'a>(
        &'a self,
        path: &wesl::syntax::ModulePath,
    ) -> Result<std::borrow::Cow<'a, str>, wesl::error::ResolveError> {
        self.0.resolve_source(&self.canonical_path(path))
    }

    /// Treats nested dependency paths as the registered crate's absolute module identity.
    fn canonical_path(&self, path: &wesl::syntax::ModulePath) -> wesl::syntax::ModulePath {
        let mut path = path.clone();
        if let wesl::syntax::PathOrigin::Package(package) = &mut path.origin
            && let Some((_, name)) = package.rsplit_once('/')
        {
            *package = name.to_owned();
        }
        path
    }
}
