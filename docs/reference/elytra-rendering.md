# Elytra rendering

Vanilla rules for the pinned Bedrock resource pack:

| State | Rule |
| --- | --- |
| Equipment | Worn in the chest slot; draw `geometry.elytra` from `models/mobs.json`. Hide the chest skin layer. |
| Texture | Use `textures/models/armor/elytra`; a player cape image replaces the base image. |
| Material | Vanilla `elytra` and `elytra_glint` derive `entity_alphatest` (`materials/entity.material`): alpha-tested and double-sided. Each pack material group keeps its own resolved settings. |
| Enchanted | Use the actor glint image and animated foil shading over the base texture. |
| Perspective | Draw on visible player bodies in third person and on other players. First person omits the worn body. |
| Standing and walking | Follow the body bone with folded wings from `animation.elytra.default`. |
| Sneaking and sleeping | Use the corresponding authored wing positions, rotations and scales. |
| Gliding | Use `animation.elytra.gliding`; descending movement direction changes wing spread and pitch. |
| Swimming | Use `animation.elytra.swimming`; wing pitch follows the owner's swim blend. |
| Transitions | Evaluate `controller.animation.elytra.default`, including one ordered transition per frame and 0.1-second shortest-path blends. Both states sample current queries; an interrupted blend starts again from the outgoing state. |

Geometry, textures and animations load from runtime carriers. The compiler retains
this legacy model without admitting obsolete humanoid models. Worn and held
attachables keep separate controller state and share the owner's movement queries.

Regression tests cover legacy model admission, chest draw and texture selection,
folded/gliding poses, cape replacement and enchanted material selection.
