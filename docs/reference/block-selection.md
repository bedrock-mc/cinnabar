# Block selection overlays

The selected block uses a black outline by default, as requested by the owner.
Outline Selection retains its native two-mode contract: enabled draws the pick
bounds, disabled draws the model's filled highlight. Untouched and older settings
inherit the outline default; explicit saved choices keep their value.

## Vanilla rules

| Selection | Behaviour |
| --- | --- |
| Regular graphics outline | Twelve unexpanded box edges, drawn as GPU line pairs. |
| Outline material | Opaque, unlit black; depth testing and depth writes enabled. A constant clip-depth displacement keeps surface edges visible. |
| Fence | Visual-height pick bounds, with directional connections; movement collision remains taller. |
| Wall | Visual-height pick bounds for posts and short/tall arms; straight runs without a post use narrower crosswise bounds. |
| Filled highlight | The selected model's actual faces. |

The outline has its own pipeline and vertex buffer. Camera movement transforms
retained world-space endpoints without rebuilding or uploading their geometry.
Losing the target clears the buffer.

Cinnabar currently expands each edge on the GPU to a two-physical-pixel stroke.
This provisional coverage compensates for its full-screen FXAA softening thin black
lines, especially diagonals. Bounds and colour retain the vanilla rules; matching
vanilla sample anti-aliasing and stroke coverage remains incomplete.

Incomplete parity: the depth-displacement witness is from the installed neighbouring
client version; its matching-backend numeric value is unresolved. Exact wall
selection insets and scaled deferred outlines still need
matching verification. Wall heights currently follow the pinned visible models.
Platform-specific factory defaults and live outline appearance remain unverified.

## Barriers

Barrier blocks keep their collision and remain available to the interaction ray.
Selection publication suppresses both the filled highlight and outline outside
Creative mode. Creative retains the existing overlay. The decision uses the
canonical block identifier in the current network ID space and the local player’s
effective game mode; it never guesses from texture visibility or cube geometry.

Tests cover sequential and hashed IDs, Survival, Adventure, Creative and unknown
modes, plus unchanged stone selection and barrier collision/picking.
Version-matched native comparison remains incomplete: the selection eligibility
path was inspected, but the remaining service-backed comparison was unavailable.
