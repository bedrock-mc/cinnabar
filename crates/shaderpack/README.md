# Shader-pack loading and configuration

`PackSource` loads text and bounded binary assets from a folder, ZIP archive or memory.
`ShaderPack` discovers options, dimension mappings and shader programs, and parses
`shaders.properties` under a caller-supplied `Environment` and option values, typing
program enable rules, buffer sizes, flips, scales, custom texture bindings, custom images
and custom uniform and variable definitions; other keys stay as raw entries.
`CustomUniforms` evaluates the custom uniform and variable expressions each frame as Iris
does (types, built-in functions, `smooth` state); nothing uploads them to shaders yet.

`ShaderPack::translate` turns a folder's programs into validated naga modules: Iris-style
includes and option overrides, pp-rs preprocessing, a compatibility-profile to GLSL 4.50
rewrite, then naga's GLSL frontend and validator. Each program carries a
`ProgramReflection` describing the bindings a render host supplies, or diagnostics located
in the pack's own files.

Java world identities come from the pinned Bedrock→Java mappings (`assets`): `IdMaps`
reads `block.properties`, `item.properties` and `entity.properties` (Iris's legacy block
ids when a pack has no block.properties) and resolves them for Bedrock block states,
entities and items; `BiomeUniforms` and the `BIOME_*`/`CAT_*`/`PPT_*` macros describe Java
biomes; `sky_fog` and `BiomeColors` give Java's sky and fog colours.

Render-host integration and in-game acceptance remain incomplete. Opening, inspecting or
translating a pack does not activate it in the client.

```sh
cargo test --locked -p shaderpack
```
