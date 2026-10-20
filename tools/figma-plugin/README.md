# Figma to JSON-UI

A Figma plugin that converts the selected frame into a Bedrock JSON-UI screen,
previews it with Cinnabar's JSON-UI renderer and exports it as a resource pack.
The conversion is deterministic Rust (`tools/jsonui-editor/src/figma`) running as
WebAssembly, so the same frame always gives the same pack. It uses only the free
plugin API.

## Build

Needs the `wasm32-unknown-unknown` target, the `wasm-bindgen` CLI version that
`Cargo.lock` pins, Node.js and npm. From the repository root:

```sh
make figma-plugin
```

This builds the editor core through `tools/jsonui-editor/build.sh`, compiles the
Cinnangles Sans font for the preview, and writes `tools/figma-plugin/dist/`.

## Load it in Figma

In the Figma desktop app, open **Plugins → Development → Import plugin from
manifest…** and choose `tools/figma-plugin/manifest.json`. Run it from
**Plugins → Development → Figma to JSON-UI**, then select a frame.

The window shows the preview, every warning (click a layer name to select it)
and the generated JSON. Selecting layers inside the converted frame keeps it as
the screen; select another frame, or use **Convert selection** to convert the
selected layer itself. **Export pack** downloads a `.mcpack` or `.zip`;
re-exporting the same frame keeps its UUIDs and bumps the version.

## What converts

Design at 1 Figma pixel per Bedrock GUI pixel. The selected frame becomes
`<frame>_screen` in the namespace named after the frame, filling the screen.

| Figma | JSON-UI |
| --- | --- |
| Frame, group, component, instance | `panel` (fills become `background` images) |
| Auto-layout frame | `stack_panel` with spacer panels; padding, alignment and absolute children add a wrapping `panel` |
| Text | `label` drawn exactly as written (not localized): font (`Minecraft Ten` → `MinecraftTen`, `Minecraft` → default, others → `smooth`), size (8 px = scale 1), colour, alignment |
| Rectangle with one fill | `image`: solid fills tint a generated white texture; image fills export their PNG |
| Rectangle with no or several fills | `panel`, one image per fill |
| Constraints | `anchor_from`/`anchor_to`, `offset` and `size` against the enclosing frame (through any groups), so the screen resizes as the frame does |
| Min and max width and height | `min_size` and `max_size` |

## Naming convention

Add tokens to a layer's name; the other words name the control.

| Token | Effect |
| --- | --- |
| `@button` or `@button=<id>` | A button pressed as `button.<id>` (default: the layer name). Children named `default`, `hover`, `pressed` or `locked` become those states, drawn over each other in the default child's place and size; a missing hover or pressed state looks like the default. Without a `default` child the content shows in every state. |
| `@image` | Exports the layer, with everything inside it, as one texture. Use it for vectors and effects. |
| `@factory=<collection>` | On an auto-layout frame: repeats its first child for each item of `<collection>`. The other children only supply preview values. |
| `$binding=#name` | Binds a text layer's text, or an image's texture, to `#name` (case kept as written). |
| `$visible=#name` | Binds the layer's visibility to `#name`. |

A `@button` layer can also take its states from Figma's own variants: when it
is a component set, or an instance of one, whose variant property `State`
(any case) has the values `Default`, `Hover`, `Pressed` and `Locked`
(`Disabled` also means locked), each variant becomes that state. An instance
uses the variants that share its other properties (such as `Size`) and keeps
its overrides; the button takes the default variant's size. Without such
variants, the child-name convention above applies.

The preview fills bindings with the values the design shows.

## Not converted

Each of these appears as a warning naming the layer: vectors, shapes and other
layer types without `@image`, rotation, blend modes, masks, effects, strokes,
corner radius, gradient fills, crop and tile image scaling, wrapping auto-layout,
space-between distribution, baseline alignment, mixed text styles, letter spacing,
fixed line height, text case, decoration, justification, truncation, and a `§` inside text (Bedrock reads it and the next character as a formatting code and has no escape). Importing JSON-UI into Figma and animations are out of
scope.
