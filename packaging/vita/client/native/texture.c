/* Base RGBA layer ownership; only the creating renderer thread may call these. */
#include <stdint.h>
#include <vitaGL.h>

void cbn_texture_release(uint32_t texture) {
    glFinish();
    glBindTexture(GL_TEXTURE_2D, texture);
    void *pixels = vglGetTexDataPointer(GL_TEXTURE_2D);
    /* This base-only texture has no palette, mip allocation or framebuffer refs.
       Detach before deleting the GL slot so VitaGL cannot enqueue a second free.
       Completion above makes immediate payload release safe for the budget. */
    vglOverloadTexDataPointer(GL_TEXTURE_2D, NULL);
    glDeleteTextures(1, &texture);
    if (pixels) vglFree(pixels);
}

uint32_t cbn_texture_upload(uint32_t size, const uint8_t *pixels, uint32_t bytes) {
    GLint max_size;
    glGetIntegerv(GL_MAX_TEXTURE_SIZE, &max_size);
    if (size < 8 || (size & (size - 1)) || size > (uint32_t)max_size ||
        (uint64_t)size * size * 4 != bytes || !pixels) return 0;
    if (glGetError() != GL_NO_ERROR) return 0;
    GLuint texture = 0;
    glGenTextures(1, &texture);
    if (!texture) return 0;
    glBindTexture(GL_TEXTURE_2D, texture);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_REPEAT);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_REPEAT);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, size, size, 0, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    if (glGetError() != GL_NO_ERROR || !vglGetTexDataPointer(GL_TEXTURE_2D)) {
        cbn_texture_release(texture);
        return 0;
    }
    return texture;
}
