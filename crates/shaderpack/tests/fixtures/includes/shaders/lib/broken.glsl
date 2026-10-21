// A helper with a mistake: it calls a function nothing defines.

vec3 sharpen(vec3 color) {
    return color + missingDetailTerm(color);
}
