# Shared client-part drawing example

This client-only sample draws an original low-poly planet and a procedural star cube.
It uses the same retained meshes and materials as a personal mod. The planet has a
6,000-block radius, placed 18,000 blocks away through a camera anchor; terrain does
not need to be loaded around it. The signed RGBA colour map is sampled by the guest
at mesh vertices. The shader hides the convex planet's far side with zero coverage.
This sample does not add a native texture-sampling material.

Build the component with:

```sh
cargo build --locked --release -p experience-space-client --target wasm32-unknown-unknown
```

Use `cinnabar-cxb build` with this directory's `experience.toml` and the resulting
`experience_space_client.wasm`, then offer the `.cxb` through the existing local
server extension helper. Seed the developer client's cache with `cinnabar-cxb
seed-cache`. The client needs the existing developer execution switch and consent
for `world_draw` and `render_material`. Production execution remains unavailable.

The build script generates `declared::assets` from the manifest and rejects missing
files. `Asset::read_range` lets other guests stream larger assets across callbacks.
The example rebuilds in `epoch()` after a world change; F9 revokes the entire scene.

The model, colour map and shader were authored for this example. They contain no
Minecraft pack data or third-party artwork.
