// Shared helpers: pulls in the curve library relative to this file.
#include "nested/curves.glsl"

vec3 gradeColor(vec3 color) {
    return vec3(softCurve(color.r), softCurve(color.g), softCurve(color.b)) * EXPOSURE_SCALE;
}
