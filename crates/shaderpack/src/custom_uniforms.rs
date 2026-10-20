//! Per-frame evaluation of the `uniform.*` and `variable.*` custom uniforms declared in
//! `shaders.properties`, following OptiFine's `shaders.txt` and Iris.
//!
//! [`CustomUniforms::new`] compiles the definitions once, checking names, function arity
//! and every type it can know statically; [`CustomUniforms::evaluate`] then runs them each
//! frame against the host's built-in uniforms without allocating. Float arithmetic uses
//! `f32` at each operation; `int` results truncate toward zero like a Java cast.

mod compile;
mod eval;
mod value;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;

use crate::{CustomUniform, CustomUniformKind};

use compile::{Binding, Compiler, Node, Shape};
use eval::{Frame, RANDOM_SEED, SmoothState};
use value::Value;

/// A typed custom or built-in uniform value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(i32),
    /// `float`.
    Float(f32),
    /// `vec2`.
    Vec2([f32; 2]),
    /// `vec3`.
    Vec3([f32; 3]),
    /// `vec4`.
    Vec4([f32; 4]),
}

/// A custom uniform or variable definition that failed to compile and was dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomUniformError {
    /// 1-based line of the definition in `shaders.properties`.
    pub line: u32,
    /// Name of the dropped uniform or variable.
    pub name: String,
    /// What went wrong, suitable for a pack diagnostic.
    pub message: String,
}

impl fmt::Display for CustomUniformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: `{}`: {}", self.line, self.name, self.message)
    }
}

impl std::error::Error for CustomUniformError {}

/// Compiled custom uniforms and variables with the state that persists between frames
/// (`smooth()` accumulators and the `random()` generator).
#[derive(Debug)]
pub struct CustomUniforms {
    /// Definitions that compiled, in dependency order.
    definitions: Vec<Definition>,
    /// Built-in names read by the definitions, in first-use order.
    inputs: Vec<String>,
    /// This frame's value of each input; `None` for a name the host does not know.
    input_values: Vec<Option<Value>>,
    /// Latest value of each definition, converted to its declared type.
    values: Vec<Value>,
    /// One accumulator per `smooth()` call site.
    smooth: Vec<SmoothState>,
    /// State of the deterministic generator behind `random()` and `randomInt()`.
    rng: u64,
    /// Latest value of each `uniform.*` definition, in definition order.
    results: Vec<(String, UniformValue)>,
}

/// One compiled definition.
#[derive(Debug)]
struct Definition {
    ty: DeclaredType,
    node: Node,
    /// Index into [`CustomUniforms::results`] for a `uniform.*`; `None` for a `variable.*`.
    result: Option<usize>,
}

impl CustomUniforms {
    /// Compiles acyclic definitions in dependency order, including forward references.
    /// Invalid definitions, cycles and their dependents are diagnosed and dropped.
    /// A repeated name keeps its first definition; results retain declaration order.
    pub fn new(definitions: &[CustomUniform]) -> (CustomUniforms, Vec<CustomUniformError>) {
        let mut first_lines = HashMap::new();
        let mut unique = Vec::new();
        let mut errors = Vec::new();
        for definition in definitions {
            if let Some(line) = first_lines.get(definition.name.as_str()) {
                errors.push(CustomUniformError {
                    line: definition.line,
                    name: definition.name.clone(),
                    message: format!("`{}` is already defined on line {line}", definition.name),
                });
            } else {
                first_lines.insert(definition.name.as_str(), definition.line);
                unique.push(definition);
            }
        }
        let (order, cyclic) = definition_order(&unique);
        let mut bindings: HashMap<String, Binding> = HashMap::new();
        let mut compiled = Vec::new();
        let mut inputs = Vec::new();
        let mut smooth_count = 0;
        for index in cyclic {
            let definition = unique[index];
            bindings.insert(definition.name.clone(), Binding::Failed);
            errors.push(CustomUniformError {
                line: definition.line,
                name: definition.name.clone(),
                message: "cyclic dependency, or depends on a cyclic definition".to_owned(),
            });
        }
        for source_index in order {
            let definition = unique[source_index];
            let name = definition.name.as_str();
            let inputs_len = inputs.len();
            let smooth_len = smooth_count;
            let mut compiler = Compiler {
                current: name,
                bindings: &bindings,
                inputs: &mut inputs,
                smooth_count: &mut smooth_count,
            };
            let outcome = compile_definition(definition, &mut compiler);
            if outcome.is_err() {
                inputs.truncate(inputs_len);
                smooth_count = smooth_len;
            }
            match outcome {
                Ok((ty, node)) => {
                    bindings.insert(
                        definition.name.clone(),
                        Binding::Defined {
                            index: compiled.len(),
                            shape: ty.shape(),
                        },
                    );
                    let result =
                        (definition.kind == CustomUniformKind::Uniform).then_some(source_index);
                    compiled.push(Definition { ty, node, result });
                }
                Err(message) => {
                    bindings
                        .entry(definition.name.clone())
                        .or_insert(Binding::Failed);
                    errors.push(CustomUniformError {
                        line: definition.line,
                        name: definition.name.clone(),
                        message,
                    });
                }
            }
        }
        let mut outputs: Vec<_> = compiled
            .iter_mut()
            .filter_map(|definition| definition.result.map(|source| (source, definition)))
            .collect();
        outputs.sort_by_key(|(source, _)| *source);
        let results = outputs
            .into_iter()
            .enumerate()
            .map(|(slot, (source, definition))| {
                definition.result = Some(slot);
                (unique[source].name.clone(), definition.ty.zero())
            })
            .collect();
        errors.sort_by_key(|error| error.line);
        let uniforms = CustomUniforms {
            values: compiled.iter().map(|d| Value::from(d.ty.zero())).collect(),
            definitions: compiled,
            input_values: vec![None; inputs.len()],
            inputs,
            smooth: vec![SmoothState::default(); smooth_count],
            rng: RANDOM_SEED,
            results,
        };
        (uniforms, errors)
    }

    /// Names of the built-in uniforms the definitions read, each once, in first-use order,
    /// so the host only computes what is needed. A vector component access such as
    /// `cameraPosition.y` reads `cameraPosition`; a matrix element such as
    /// `gbufferModelView.3.1` (GLSL `gbufferModelView[3][1]`) is read by its full dotted
    /// name and must be answered with a scalar.
    pub fn inputs(&self) -> impl Iterator<Item = &str> {
        self.inputs.iter().map(String::as_str)
    }

    /// Forgets every `smooth()` accumulator, so each call next returns its current value,
    /// as on the first evaluation. Iris builds a pack's uniforms when the world starts
    /// rendering; a host that evaluates them earlier (before the player's biome is known)
    /// restarts here so smoothing starts from world values, not placeholders.
    pub fn restart_smoothing(&mut self) {
        self.smooth.fill(SmoothState::default());
    }

    /// Evaluates every definition in order and returns the `uniform.*` results (not
    /// `variable.*`) in definition order.
    ///
    /// `builtin(name)` is called once per [`inputs`](Self::inputs) name and returns that
    /// built-in's value (e.g. `"cameraPosition"` → `Vec3`, `"frameTimeCounter"` → `Float`,
    /// `"isEyeInWater"` → `Int`); `None` evaluates as 0. `frame_time` is the seconds since
    /// the previous call and drives `smooth()`; a negative or non-finite value counts as 0.
    /// A definition whose value has the wrong type at run time (a built-in of unexpected
    /// type) yields its type's zero value.
    pub fn evaluate(
        &mut self,
        builtin: &dyn Fn(&str) -> Option<UniformValue>,
        frame_time: f32,
    ) -> &[(String, UniformValue)] {
        for (slot, name) in self.input_values.iter_mut().zip(&self.inputs) {
            *slot = builtin(name).map(Value::from);
        }
        let elapsed = if frame_time.is_finite() && frame_time > 0.0 {
            frame_time
        } else {
            0.0
        };
        for (index, definition) in self.definitions.iter().enumerate() {
            let (earlier, rest) = self.values.split_at_mut(index);
            let mut frame = Frame {
                inputs: &self.input_values,
                values: earlier,
                smooth: &mut self.smooth,
                rng: &mut self.rng,
                elapsed,
            };
            let converted = definition
                .node
                .eval(&mut frame)
                .ok()
                .and_then(|value| definition.ty.convert(value))
                .unwrap_or_else(|| definition.ty.zero());
            if let Some(slot) = rest.first_mut() {
                *slot = Value::from(converted);
            }
            if let Some(result) = definition.result
                && let Some(entry) = self.results.get_mut(result)
            {
                entry.1 = converted;
            }
        }
        &self.results
    }
}

/// Orders definitions after their dependencies and returns unresolved cycles and dependents.
fn definition_order(definitions: &[&CustomUniform]) -> (Vec<usize>, Vec<usize>) {
    let names: HashMap<_, _> = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.name.as_str(), index))
        .collect();
    let mut pending = vec![0; definitions.len()];
    let mut dependents = vec![Vec::new(); definitions.len()];
    for (index, definition) in definitions.iter().enumerate() {
        let mut dependencies = Vec::new();
        for name in definition.expr.identifiers() {
            let base = name.split('.').next().unwrap_or(name);
            if let Some(&dependency) = names.get(base)
                && !dependencies.contains(&dependency)
            {
                dependencies.push(dependency);
                dependents[dependency].push(index);
            }
        }
        pending[index] = dependencies.len();
    }
    let mut order: Vec<_> = pending
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect();
    let mut cursor = 0;
    while cursor < order.len() {
        let dependency = order[cursor];
        cursor += 1;
        for &dependent in &dependents[dependency] {
            pending[dependent] -= 1;
            if pending[dependent] == 0 {
                order.push(dependent);
            }
        }
    }
    let cyclic = pending
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count != 0).then_some(index))
        .collect();
    (order, cyclic)
}

/// Compiles one definition's type and expression, checking the expression fits the type.
fn compile_definition(
    definition: &CustomUniform,
    compiler: &mut Compiler<'_>,
) -> Result<(DeclaredType, Node), String> {
    let Some(ty) = DeclaredType::parse(&definition.ty) else {
        return Err(format!(
            "unsupported type `{}` (expected float, int, bool, vec2, vec3 or vec4)",
            definition.ty
        ));
    };
    let (node, shape) = compiler.compile(&definition.expr)?;
    match (ty.shape(), shape) {
        (_, Shape::Dynamic) => Ok((ty, node)),
        (declared, found) if declared == found => Ok((ty, node)),
        (_, found) => Err(format!(
            "the expression is {found} but the {} is declared `{}`",
            match definition.kind {
                CustomUniformKind::Uniform => "uniform",
                CustomUniformKind::Variable => "variable",
            },
            definition.ty
        )),
    }
}

/// The declared type of a definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeclaredType {
    Bool,
    Int,
    Float,
    /// `vec2`, `vec3` or `vec4` by component count.
    Vector(u8),
}

impl DeclaredType {
    /// Parses the type token of `uniform.<type>.<name>`; `None` if unsupported.
    fn parse(token: &str) -> Option<Self> {
        Some(match token {
            "bool" => Self::Bool,
            "int" => Self::Int,
            "float" => Self::Float,
            "vec2" => Self::Vector(2),
            "vec3" => Self::Vector(3),
            "vec4" => Self::Vector(4),
            _ => return None,
        })
    }

    /// The static shape a value of this type has.
    fn shape(self) -> Shape {
        match self {
            Self::Vector(len) => Shape::Vector(len),
            Self::Bool | Self::Int | Self::Float => Shape::Scalar,
        }
    }

    /// The value used before the first evaluation and when evaluation fails.
    fn zero(self) -> UniformValue {
        match self {
            Self::Bool => UniformValue::Bool(false),
            Self::Int => UniformValue::Int(0),
            Self::Float => UniformValue::Float(0.0),
            Self::Vector(2) => UniformValue::Vec2([0.0; 2]),
            Self::Vector(3) => UniformValue::Vec3([0.0; 3]),
            Self::Vector(_) => UniformValue::Vec4([0.0; 4]),
        }
    }

    /// Converts an evaluated value to this type: scalars convert freely (`int` truncates
    /// toward zero and saturates like a Java cast, `bool` is non-zero), vectors must have
    /// exactly this many components. `None` on a shape mismatch.
    fn convert(self, value: Value) -> Option<UniformValue> {
        match self {
            Self::Bool => value.truth().map(UniformValue::Bool),
            Self::Int => value.integer().map(UniformValue::Int),
            Self::Float => value.number().map(UniformValue::Float),
            Self::Vector(len) => {
                let Value::Vector { len: found, items } = value else {
                    return None;
                };
                if found != len {
                    return None;
                }
                let [x, y, z, w] = items;
                Some(match len {
                    2 => UniformValue::Vec2([x, y]),
                    3 => UniformValue::Vec3([x, y, z]),
                    _ => UniformValue::Vec4([x, y, z, w]),
                })
            }
        }
    }
}
