// Curve helpers; constants come from an absolute include.
#include "/lib/constants.glsl"

float softCurve(float x) {
    return x / (x + CURVE_KNEE);
}
