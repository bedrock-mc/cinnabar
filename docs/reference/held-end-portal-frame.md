# Held End portal frame

The world carrier already compiles the Bedrock portal-frame base as a 13/16-block cuboid,
with the direction-specific top UV rotation and independent top, side and bottom materials.
The carried item uses the no-eye state selected by the shared block-item registry route.
The vanilla pack's `carried_textures` entry is the eye texture used by the world model's
optional eye cuboid; it is not a six-face override for a held frame.

Reuse the exact compiled no-eye template and its face materials in the equipment atlas.
Retain its cropped UVs, winding and bounds beneath the existing held-block placement.
The six-face cube fallback and flat inventory icon are unsuitable for this shape.
