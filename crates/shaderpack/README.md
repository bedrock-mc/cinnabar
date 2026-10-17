# Shader-pack translation

`shaderpack` loads Java Edition OptiFine/Iris shader packs from a folder, zip or
in-memory files. It translates vertex, fragment and compute stages to validated
naga modules and reports the bindings each program needs. It has no Bevy or wgpu
dependency.

Open a pack with `ShaderPack::open`, inspect `ShaderPack::options`, and pass option
overrides and an `Environment` through `TranslateConfig`. Use
`ShaderPack::dimension_folder` to select a dimension's folder, then call
`ShaderPack::translate`. Each `ProgramTranslation` carries stage results,
diagnostics and, on success, a `ProgramReflection` describing vertex inputs,
uniform layouts, textures, samplers, storage resources, colour outputs and compute
dispatch.

The translation includes relative and absolute shader includes, macro
preprocessing, legacy GLSL rewrites, active buffer directives, program enable
rules and dimension-folder fallback. `shaders.properties` supplies typed rules
and parsed custom uniform expressions. A failed program keeps its diagnostics
while other programs can still translate.

Try the command-line report with an original fixture pack:

```sh
cargo run -p shaderpack --example translate -- crates/shaderpack/tests/fixtures/legacy --verbose
```

The example also accepts a pack zip, `--folder world0`, `--enabled-only` and
`--no-dh`. By default it attempts disabled programs with option forcing and
includes Distant Horizons programs. The library defaults to enabled programs
without Distant Horizons. The example prints a per-program summary; translation
failures in that summary do not change its exit status.

Successful translation establishes a module and binding contract. A render host
must check device capabilities and limits, provide resources and uniform values,
and implement the pack's render passes and property rules. This crate does not
activate packs in the client or establish that they render correctly in game.
Geometry and tessellation stages produce unsupported-stage diagnostics.

The original fixtures under `tests/fixtures` cover legacy shaders, includes and
located errors, options, buffer directives, compute resources and dimension
folders. Integration tests also compare folder and zip translation without a GPU.
Run these tests with `cargo test -p shaderpack`.
