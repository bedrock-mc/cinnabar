# Shader-pack loading and configuration

`PackSource` loads text and bounded binary assets from a folder, ZIP archive or memory.
`ShaderPack` discovers options, dimension mappings and shader programs, and parses
`shaders.properties` under a caller-supplied `Environment` and option values, typing
program enable rules, buffer sizes, flips, scales, custom texture bindings, custom images
and custom uniform and variable definitions; other keys stay as raw entries. Custom uniform
expressions are parsed, not evaluated.

Shader translation, render-host integration and in-game acceptance remain incomplete.
Opening or inspecting a pack does not activate it in the client.

```sh
cargo test --locked -p shaderpack
```
