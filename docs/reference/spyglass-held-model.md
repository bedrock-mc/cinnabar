# Held spyglass model

The pinned resource pack includes `models/entity/spyglass.geo.json`,
`textures/entity/spyglass.png`, and the `animation.spyglass.holding` and
`animation.spyglass.scoping` clips. It supplies no spyglass attachable definition.
The ordinary inventory icon is not the held model shown in the vanilla reference in issue #264.

The startup compiler adds original binding metadata connecting those existing pack references.
It selects holding while the owner is not using the item and scoping in third person while
use duration is positive. The first-person scoped view does not select the scoping clip.
The model's own binding expression then selects the main-hand or head bone, and its authored
holding clip distinguishes first and third person. Authored attachables retain their binding;
server pack compilation does not insert this startup completion.

Third-person held models with a dynamic pose use the same animation evaluation and root-bone
composition as first-person attachables. Literal third-person grips retain their existing path.
