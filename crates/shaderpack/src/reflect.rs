//! Builds a [`ProgramReflection`] from a program's validated stage modules.

use crate::buffers::{BufferFormat, BufferId};
use crate::compile::CompiledStage;
use crate::programs::{ProgramKind, StageKind};
use crate::reflection::{
    BufferBinding, ColorOutput, ComputeDispatch, LOOSE_UNIFORM_BINDING, LOOSE_UNIFORM_BLOCK,
    ProgramReflection, SampleType, SamplerBinding, ScalarType, StageMask, StorageImageBinding,
    TextureBinding, TextureDimension, UNIFORM_GROUP, UniformBlock, UniformMember, ValueType,
    VertexAttribute, VertexInput,
};
use naga::{AddressSpace, ArraySize, Binding, ImageClass, ImageDimension, ScalarKind, TypeInner};
use std::collections::BTreeMap;

/// What the transformer knows about a stage beyond its naga module.
pub(crate) struct StageNames<'a> {
    pub vertex_inputs: &'a [(u32, String)],
    pub frag_outputs: &'a [(u32, String)],
    pub resources: &'a [(u32, u32, String)],
}

/// A binding seen in one or more stages, before sorting into the reflection lists.
enum Resource {
    Uniform(BufferBinding, Option<UniformBlock>),
    Storage(BufferBinding),
    Texture(TextureBinding),
    Sampler(SamplerBinding),
    Image(StorageImageBinding),
}

/// Reflects `stages` (pipeline order). `draw_buffers` maps output locations to buffer
/// indices. Formats start at the default; the caller sets them from pack-wide directives.
/// Errors describe cross-stage binding conflicts.
pub(crate) fn reflect(
    kind: ProgramKind,
    stages: &[(&CompiledStage, StageNames<'_>)],
    draw_buffers: &[u32],
) -> Result<ProgramReflection, String> {
    crate::reflect_link::validate(&stages.iter().map(|(stage, _)| *stage).collect::<Vec<_>>())?;
    let mut out = ProgramReflection {
        kind,
        entry_points: vec![],
        vertex_inputs: vec![],
        uniforms: None,
        uniform_buffers: vec![],
        storage_buffers: vec![],
        textures: vec![],
        samplers: vec![],
        storage_images: vec![],
        color_outputs: vec![],
        writes_depth: false,
        compute: None,
    };
    let mut resources: BTreeMap<(u32, u32), Resource> = BTreeMap::new();
    for (compiled, names) in stages {
        let module = &compiled.module;
        let Some(ep) = module.entry_points.first() else {
            return Err(format!(
                "{} stage has no entry point",
                compiled.stage.extension()
            ));
        };
        out.entry_points.push((compiled.stage, ep.name.clone()));
        let usage = compiled.info.get_entry_point(0);
        for (handle, var) in module.global_variables.iter() {
            let Some(rb) = &var.binding else { continue };
            if usage[handle].is_empty() {
                continue;
            }
            let name = names
                .resources
                .iter()
                .find(|(g, b, _)| *g == rb.group && *b == rb.binding)
                .map(|(_, _, n)| n.clone())
                .or_else(|| var.name.clone())
                .unwrap_or_default();
            let res = resource(module, var, name, rb.group, rb.binding, compiled.stage)?;
            match resources.get_mut(&(rb.group, rb.binding)) {
                Some(existing) => merge(existing, res, rb)?,
                None => {
                    resources.insert((rb.group, rb.binding), res);
                }
            }
        }
        let args = ep
            .function
            .arguments
            .iter()
            .map(|a| (a.ty, a.binding.clone()));
        let result = ep.function.result.iter().map(|r| (r.ty, r.binding.clone()));
        match compiled.stage {
            StageKind::Vertex => {
                for (location, ty) in locations(module, args) {
                    let name = names
                        .vertex_inputs
                        .iter()
                        .find(|(l, _)| *l == location)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    out.vertex_inputs.push(VertexInput {
                        location,
                        attribute: VertexAttribute::from_glsl_name(&name),
                        name,
                        ty,
                    });
                }
            }
            StageKind::Fragment => {
                let result: Vec<_> = result.collect();
                out.writes_depth = result
                    .iter()
                    .any(|(ty, b)| has_builtin(module, *ty, b, naga::BuiltIn::FragDepth));
                for (location, ty) in locations(module, result) {
                    let name = names
                        .frag_outputs
                        .iter()
                        .find(|(l, _)| *l == location)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    let index = draw_buffers.get(location as usize).copied();
                    out.color_outputs.push(ColorOutput {
                        location,
                        name,
                        ty,
                        buffer: index,
                        format: BufferFormat::DEFAULT,
                    });
                }
            }
            StageKind::Compute => {
                out.compute = Some(ComputeDispatch {
                    workgroup_size: ep.workgroup_size,
                    work_groups: constant(module, "workGroups").and_then(|v| v.as_u32s::<3>()),
                    work_groups_render: constant(module, "workGroupsRender")
                        .and_then(|v| v.as_f32s::<2>()),
                });
            }
            _ => {}
        }
    }
    for (_, res) in resources {
        match res {
            Resource::Uniform(b, Some(block))
                if b.group == UNIFORM_GROUP && b.binding == LOOSE_UNIFORM_BINDING =>
            {
                out.uniforms = Some(block);
            }
            Resource::Uniform(b, _) => out.uniform_buffers.push(b),
            Resource::Storage(b) => out.storage_buffers.push(b),
            Resource::Texture(t) => out.textures.push(t),
            Resource::Sampler(s) => out.samplers.push(s),
            Resource::Image(i) => out.storage_images.push(i),
        }
    }
    Ok(out)
}

/// The buffer a colour output lands in.
pub(crate) fn output_buffer(index: u32, shadow: bool) -> BufferId {
    if shadow {
        BufferId::ShadowColor(index)
    } else {
        BufferId::Color(index)
    }
}

/// Creates a visibility mask for one compiled stage.

fn mask(stage: StageKind) -> StageMask {
    let mut m = StageMask::default();
    m.add(stage);
    m
}

/// Reflects one bound global and its host-visible layout.

fn resource(
    module: &naga::Module,
    var: &naga::GlobalVariable,
    name: String,
    group: u32,
    binding: u32,
    stage: StageKind,
) -> Result<Resource, String> {
    let stages = mask(stage);
    let inner = &module.types[var.ty].inner;
    let size = inner.size(module.to_ctx());
    Ok(match var.space {
        AddressSpace::Uniform => {
            let block = (group == UNIFORM_GROUP && binding == LOOSE_UNIFORM_BINDING).then(|| {
                let mut members = vec![];
                flatten_uniforms(module, var.ty, String::new(), 0, &mut members);
                UniformBlock {
                    name: LOOSE_UNIFORM_BLOCK.to_string(),
                    group,
                    binding,
                    size,
                    members,
                    stages,
                }
            });
            Resource::Uniform(
                BufferBinding {
                    name,
                    group,
                    binding,
                    min_size: size,
                    read_only: true,
                    stages,
                },
                block,
            )
        }
        AddressSpace::Storage { access } => Resource::Storage(BufferBinding {
            name,
            group,
            binding,
            min_size: size,
            read_only: !access.contains(naga::StorageAccess::STORE),
            stages,
        }),
        AddressSpace::Handle => match inner {
            TypeInner::Sampler { comparison } => Resource::Sampler(SamplerBinding {
                name,
                group,
                binding,
                comparison: *comparison,
                stages,
            }),
            TypeInner::Image {
                dim,
                arrayed,
                class,
            } => {
                let dimension = dimension(*dim, *arrayed);
                match class {
                    ImageClass::Sampled { kind, multi } => Resource::Texture(TextureBinding {
                        name,
                        group,
                        binding,
                        dimension,
                        sample_type: match kind {
                            ScalarKind::Sint => SampleType::Sint,
                            ScalarKind::Uint => SampleType::Uint,
                            _ => SampleType::Float,
                        },
                        multisampled: *multi,
                        stages,
                    }),
                    ImageClass::Depth { multi } => Resource::Texture(TextureBinding {
                        name,
                        group,
                        binding,
                        dimension,
                        sample_type: SampleType::Depth,
                        multisampled: *multi,
                        stages,
                    }),
                    ImageClass::Storage { format, access } => {
                        Resource::Image(StorageImageBinding {
                            name,
                            group,
                            binding,
                            dimension,
                            format: *format,
                            read: access.contains(naga::StorageAccess::LOAD),
                            write: access.contains(naga::StorageAccess::STORE),
                            atomic: access.contains(naga::StorageAccess::ATOMIC),
                            stages,
                        })
                    }
                    ImageClass::External => return Err(format!("`{name}` is an external texture")),
                }
            }
            other => return Err(format!("`{name}` has unexpected handle type {other:?}")),
        },
        other => return Err(format!("`{name}` is in unexpected address space {other:?}")),
    })
}

/// Combines a binding seen by another stage; the declarations must agree.
fn merge(existing: &mut Resource, new: Resource, rb: &naga::ResourceBinding) -> Result<(), String> {
    let conflict = || {
        format!(
            "stages disagree on the resource at group {} binding {}",
            rb.group, rb.binding
        )
    };
    match (existing, new) {
        (Resource::Uniform(a, ablock), Resource::Uniform(b, bblock)) => {
            if a.min_size != b.min_size
                || ablock.as_ref().map(|x| &x.members) != bblock.as_ref().map(|x| &x.members)
            {
                return Err(conflict());
            }
            union(&mut a.stages, b.stages);
            if let Some(block) = ablock {
                union(&mut block.stages, b.stages);
            }
        }
        (Resource::Storage(a), Resource::Storage(b)) if a.min_size == b.min_size => {
            a.read_only &= b.read_only;
            union(&mut a.stages, b.stages);
        }
        (Resource::Texture(a), Resource::Texture(b))
            if (a.dimension, a.sample_type, a.multisampled)
                == (b.dimension, b.sample_type, b.multisampled) =>
        {
            union(&mut a.stages, b.stages);
        }
        (Resource::Sampler(a), Resource::Sampler(b)) if a.comparison == b.comparison => {
            union(&mut a.stages, b.stages);
        }
        (Resource::Image(a), Resource::Image(b))
            if (a.dimension, a.format) == (b.dimension, b.format) =>
        {
            a.read |= b.read;
            a.write |= b.write;
            a.atomic |= b.atomic;
            union(&mut a.stages, b.stages);
        }
        _ => return Err(conflict()),
    }
    Ok(())
}

/// Adds every stage in the source mask to the destination mask.

fn union(a: &mut StageMask, b: StageMask) {
    a.vertex |= b.vertex;
    a.fragment |= b.fragment;
    a.compute |= b.compute;
}

/// Converts a naga image shape to the host texture dimension.

fn dimension(dim: ImageDimension, arrayed: bool) -> TextureDimension {
    match (dim, arrayed) {
        (ImageDimension::D1, false) => TextureDimension::D1,
        (ImageDimension::D1, true) => TextureDimension::D1Array,
        (ImageDimension::D2, false) => TextureDimension::D2,
        (ImageDimension::D2, true) => TextureDimension::D2Array,
        (ImageDimension::D3, _) => TextureDimension::D3,
        (ImageDimension::Cube, false) => TextureDimension::Cube,
        (ImageDimension::Cube, true) => TextureDimension::CubeArray,
    }
}

/// Converts a naga scalar to its reflected component type.

fn scalar(s: naga::Scalar) -> ScalarType {
    match (s.kind, s.width) {
        (ScalarKind::Bool, _) => ScalarType::Bool,
        (ScalarKind::Sint, 1) => ScalarType::Int8,
        (ScalarKind::Sint, 2) => ScalarType::Int16,
        (ScalarKind::Sint | ScalarKind::AbstractInt, 4) => ScalarType::Int,
        (ScalarKind::Sint | ScalarKind::AbstractInt, 8) => ScalarType::Int64,
        (ScalarKind::Uint, 1) => ScalarType::Uint8,
        (ScalarKind::Uint, 2) => ScalarType::Uint16,
        (ScalarKind::Uint, 4) => ScalarType::Uint,
        (ScalarKind::Uint, 8) => ScalarType::Uint64,
        (ScalarKind::Float, 2) => ScalarType::Half,
        (ScalarKind::Float, 4) => ScalarType::Float,
        (ScalarKind::Float | ScalarKind::AbstractFloat, 8) => ScalarType::Double,
        _ => unreachable!("validated scalar width: {s:?}"),
    }
}

/// Describes scalar, vector and matrix interface values.

pub(crate) fn value_type(inner: &TypeInner) -> Option<ValueType> {
    Some(match inner {
        TypeInner::Scalar(s) => ValueType::Scalar(scalar(*s)),
        TypeInner::Vector { size, scalar: s } => ValueType::Vector {
            size: *size as u8,
            scalar: scalar(*s),
        },
        TypeInner::Matrix {
            columns,
            rows,
            scalar: s,
        } => ValueType::Matrix {
            columns: *columns as u8,
            rows: *rows as u8,
            scalar: scalar(*s),
        },
        _ => return None,
    })
}

/// Expands a uniform type into leaves with absolute offsets.
fn flatten_uniforms(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    path: String,
    offset: u32,
    out: &mut Vec<UniformMember>,
) {
    match &module.types[ty].inner {
        TypeInner::Struct { members, .. } => {
            for m in members {
                let name = m.name.clone().unwrap_or_default();
                let p = if path.is_empty() {
                    name
                } else {
                    format!("{path}.{name}")
                };
                flatten_uniforms(module, m.ty, p, offset + m.offset, out);
            }
        }
        TypeInner::Array {
            base,
            size: ArraySize::Constant(n),
            stride,
        } => match value_type(&module.types[*base].inner) {
            Some(ty) => out.push(UniformMember {
                name: path,
                offset,
                ty,
                array: Some((n.get(), *stride)),
            }),
            None => {
                for i in 0..n.get() {
                    flatten_uniforms(
                        module,
                        *base,
                        format!("{path}[{i}]"),
                        offset + i * stride,
                        out,
                    );
                }
            }
        },
        inner => {
            if let Some(ty) = value_type(inner) {
                out.push(UniformMember {
                    name: path,
                    offset,
                    ty,
                    array: None,
                });
            }
        }
    }
}

/// User locations of entry point arguments or results as (location, type), sorted.
fn locations(
    module: &naga::Module,
    items: impl IntoIterator<Item = (naga::Handle<naga::Type>, Option<Binding>)>,
) -> Vec<(u32, ValueType)> {
    let mut out = vec![];
    for (ty, b) in items {
        match (&module.types[ty].inner, b) {
            (TypeInner::Struct { members, .. }, None) => {
                for m in members {
                    if let Some(Binding::Location { location, .. }) = m.binding
                        && let Some(v) = value_type(&module.types[m.ty].inner)
                    {
                        out.push((location, v));
                    }
                }
            }
            (inner, Some(Binding::Location { location, .. })) => {
                if let Some(v) = value_type(inner) {
                    out.push((location, v));
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|(l, _)| *l);
    out
}

/// Finds a builtin in direct or structured entry-point values.

fn has_builtin(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    b: &Option<Binding>,
    which: naga::BuiltIn,
) -> bool {
    match (&module.types[ty].inner, b) {
        (TypeInner::Struct { members, .. }, None) => members
            .iter()
            .any(|m| m.binding == Some(Binding::BuiltIn(which))),
        (_, Some(Binding::BuiltIn(x))) => *x == which,
        _ => false,
    }
}

/// Literal components of a module-scope constant.
struct ConstValue(Vec<naga::Literal>);

impl ConstValue {
    /// Reads a fixed-size constant containing nonnegative integer components.
    fn as_u32s<const N: usize>(&self) -> Option<[u32; N]> {
        let v: Option<Vec<u32>> = self
            .0
            .iter()
            .map(|l| match l {
                naga::Literal::I32(i) => u32::try_from(*i).ok(),
                naga::Literal::U32(u) => Some(*u),
                _ => None,
            })
            .collect();
        v?.try_into().ok()
    }

    /// Reads a fixed-size numeric constant as floating-point components.

    fn as_f32s<const N: usize>(&self) -> Option<[f32; N]> {
        let v: Option<Vec<f32>> = self
            .0
            .iter()
            .map(|l| match l {
                naga::Literal::F32(f) => Some(*f),
                naga::Literal::F64(f) => Some(*f as f32),
                _ => None,
            })
            .collect();
        v?.try_into().ok()
    }
}

/// Evaluates the named module constant if it is built from literals.
fn constant(module: &naga::Module, name: &str) -> Option<ConstValue> {
    let (_, c) = module
        .constants
        .iter()
        .find(|(_, c)| c.name.as_deref() == Some(name))?;
    let mut out = vec![];
    literals(module, c.init, &mut out)?;
    Some(ConstValue(out))
}

/// Collects scalar components from a constant expression.

fn literals(
    module: &naga::Module,
    expr: naga::Handle<naga::Expression>,
    out: &mut Vec<naga::Literal>,
) -> Option<()> {
    match &module.global_expressions[expr] {
        naga::Expression::Literal(l) => out.push(*l),
        naga::Expression::Compose { components, .. } => {
            for c in components {
                literals(module, *c, out)?;
            }
        }
        naga::Expression::Splat { size, value } => {
            for _ in 0..*size as u8 {
                literals(module, *value, out)?;
            }
        }
        naga::Expression::Constant(c) => literals(module, module.constants[*c].init, out)?,
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{OutputMap, compile};
    use crate::diagnostics::{Diagnostic, Phase};
    use crate::preprocess::LineMap;
    use crate::reflection::{SAMPLER_BINDING, TEXTURE_GROUP};

    /// Compiles hand-written Vulkan GLSL whose lines all count as generated.
    fn compile_glsl(stage: StageKind, glsl: &str) -> Result<CompiledStage, Vec<Diagnostic>> {
        let origins = vec![0; glsl.lines().count()];
        let lines = LineMap::default();
        compile(
            stage,
            &OutputMap {
                glsl,
                origins: &origins,
                lines: &lines,
            },
        )
    }

    const FRAGMENT: &str = "#version 450
layout(set = 0, binding = 0) uniform IrisUniforms { float frameTimeCounter; vec3 cameraPosition; };
layout(set = 1, binding = 0) uniform sampler iris_sampler;
layout(set = 1, binding = 5) uniform texture2D colortex0;
layout(location = 0) out vec4 outColor;
void main() {
    vec4 base = texture(sampler2D(colortex0, iris_sampler), vec2(frameTimeCounter));
    outColor = base + vec4(cameraPosition, 0.0);
}
";

    #[test]
    fn reflection_keeps_array_dimensions_distinct() {
        assert_ne!(
            dimension(ImageDimension::D1, false),
            dimension(ImageDimension::D1, true)
        );
        assert_ne!(
            dimension(ImageDimension::D2, false),
            dimension(ImageDimension::D2, true)
        );
        assert_ne!(
            dimension(ImageDimension::Cube, false),
            dimension(ImageDimension::Cube, true)
        );
    }

    #[test]
    fn reflection_keeps_component_widths_distinct() {
        for kind in [ScalarKind::Sint, ScalarKind::Uint, ScalarKind::Float] {
            let widths: &[u8] = if kind == ScalarKind::Float {
                &[2, 4, 8]
            } else {
                &[1, 2, 4, 8]
            };
            for (i, a) in widths.iter().enumerate() {
                for b in &widths[i + 1..] {
                    assert_ne!(
                        scalar(naga::Scalar { kind, width: *a }),
                        scalar(naga::Scalar { kind, width: *b }),
                        "{kind:?}: {a} and {b}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_compiled_fragment_stage_reflects_its_uniforms_textures_and_outputs() {
        let stage = compile_glsl(StageKind::Fragment, FRAGMENT).expect("fragment compiles");
        let names = StageNames {
            vertex_inputs: &[],
            frag_outputs: &[(0, "outColor".to_owned())],
            resources: &[],
        };
        let reflection = reflect(ProgramKind::Render, &[(&stage, names)], &[3]).unwrap();
        let uniforms = reflection.uniforms.expect("loose uniform block");
        let members: Vec<_> = uniforms
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.offset))
            .collect();
        assert_eq!(members, [("frameTimeCounter", 0), ("cameraPosition", 16)]);
        assert!(uniforms.stages.fragment && !uniforms.stages.vertex);
        assert_eq!(reflection.samplers.len(), 1);
        assert_eq!(
            (reflection.samplers[0].group, reflection.samplers[0].binding),
            (TEXTURE_GROUP, SAMPLER_BINDING)
        );
        let texture = &reflection.textures[0];
        assert_eq!(texture.name, "colortex0");
        assert_eq!(texture.dimension, TextureDimension::D2);
        assert_eq!(texture.sample_type, SampleType::Float);
        let output = &reflection.color_outputs[0];
        assert_eq!((output.location, output.name.as_str()), (0, "outColor"));
        assert_eq!(output.buffer, Some(3));
        assert!(!reflection.writes_depth);
    }

    #[test]
    fn a_compute_stage_reflects_its_workgroup_constants() {
        let glsl = "#version 450
layout(local_size_x = 8, local_size_y = 4) in;
const ivec3 workGroups = ivec3(4, 2, 1);
layout(set = 2, binding = 0, rgba8) uniform writeonly image2D colorimg0;
void main() { imageStore(colorimg0, ivec2(gl_GlobalInvocationID.xy), vec4(1.0)); }
";
        let stage = compile_glsl(StageKind::Compute, glsl).expect("compute compiles");
        let names = StageNames {
            vertex_inputs: &[],
            frag_outputs: &[],
            resources: &[],
        };
        let reflection = reflect(ProgramKind::Compute, &[(&stage, names)], &[]).unwrap();
        let dispatch = reflection.compute.expect("compute dispatch");
        assert_eq!(dispatch.workgroup_size, [8, 4, 1]);
        assert_eq!(dispatch.work_groups, Some([4, 2, 1]));
        let image = &reflection.storage_images[0];
        assert_eq!(image.name, "colorimg0");
        assert!(image.write && !image.read);
    }

    #[test]
    fn a_parse_error_in_generated_code_carries_the_offending_line() {
        let glsl = "#version 450\nlayout(location = 0) out vec4 outColor;\nvoid main() { outColor = undeclared; }\n";
        let errors = compile_glsl(StageKind::Fragment, glsl).expect_err("undeclared name");
        assert_eq!(errors[0].phase, Phase::Parse);
        assert_eq!(errors[0].location, None);
        assert_eq!(
            errors[0].snippet.as_deref(),
            Some("void main() { outColor = undeclared; }")
        );
    }
}
