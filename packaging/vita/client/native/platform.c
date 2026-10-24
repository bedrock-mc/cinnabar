/* Native Vita ABI boundary. World, input policy and mesh conversion remain Rust-owned. */
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <psp2/ctrl.h>
#include <psp2/display.h>
#include <psp2/kernel/processmgr.h>
#include <psp2/kernel/threadmgr.h>
#include <psp2/power.h>
#include <vitaGL.h>

#ifndef __ARM_PCS_VFP
#error Cinnabar requires the native hard-float SDK, not the Android softfp SDK.
#endif

/* The same table generates Rust controls, so neither side can drift silently. */
#define CBN_BUTTON(name, sdk, value) _Static_assert(sdk == value, #name " SDK mask");
#include "pad_masks.def"
#undef CBN_BUTTON

unsigned int _newlib_heap_size_user = 128 * 1024 * 1024;
unsigned int _sceLibcHeapSize = 4 * 1024 * 1024;
unsigned int _sceKernelMainThreadStackSize = 1024 * 1024;

typedef struct { uint32_t buttons; uint8_t lx, ly, rx, ry; } CbnPad;
typedef struct { float position[3], uv[2]; } CbnVertex;
typedef struct { void *positions, *uvs, *indices; uint32_t count; } CbnMesh;
enum { CBN_WIDTH = 960, CBN_HEIGHT = 544 };
_Static_assert(sizeof(CbnPad) == 8, "Rust pad ABI");
_Static_assert(sizeof(CbnVertex) == 20, "Rust vertex ABI");

int cbn_init(uint32_t main_bytes, uint32_t cdram_bytes) {
    sceCtrlSetSamplingMode(SCE_CTRL_MODE_ANALOG);
    /* No overclock beyond the SDK's normal 444 MHz setting. */
    scePowerSetArmClockFrequency(444);
    scePowerSetBusClockFrequency(222);
    scePowerSetGpuClockFrequency(222);
    scePowerSetGpuXbarClockFrequency(166);
    vglSetCircularPoolSize(2 * 1024 * 1024);
    vglUseExtraMem(GL_FALSE);
    /* The return value reports resolution fallback, NOT initialization success.
       GL_FALSE is the normal result when the requested display size is accepted. */
    (void)vglInitWithCustomSizes(0, CBN_WIDTH, CBN_HEIGHT, main_bytes, cdram_bytes, 0, 0, SCE_GXM_MULTISAMPLE_NONE);
    GLint viewport[4];
    glGetIntegerv(GL_VIEWPORT, viewport);
    if (viewport[2] != CBN_WIDTH || viewport[3] != CBN_HEIGHT) return 0;
    glEnable(GL_DEPTH_TEST);
    glEnable(GL_CULL_FACE);
    glCullFace(GL_BACK);
    glEnableClientState(GL_VERTEX_ARRAY);
    vglWaitVblankStart(GL_TRUE);
    return 1;
}

uint64_t cbn_time_us(void) { return sceKernelGetProcessTimeWide(); }

int cbn_poll(CbnPad *out) {
    SceCtrlData data;
    if (sceCtrlPeekBufferPositive(0, &data, 1) <= 0) return 0;
    *out = (CbnPad){data.buttons, data.lx, data.ly, data.rx, data.ry};
    return 1;
}

/* Explicit RAM pool selection lets the Rust admission lease account for ownership.
   Split streams permit vgl's mapped path: no per-frame vertex/index copies. */
void *cbn_upload(const CbnVertex *vertices, uint32_t vertex_count,
                 const uint16_t *indices, uint32_t index_count) {
    CbnMesh *mesh = calloc(1, sizeof(*mesh));
    if (!mesh) return NULL;
    mesh->positions = vglAlloc(vertex_count * 3 * sizeof(float), VGL_MEM_RAM);
    mesh->uvs = vglAlloc(vertex_count * 2 * sizeof(float), VGL_MEM_RAM);
    mesh->indices = vglAlloc(index_count * sizeof(uint16_t), VGL_MEM_RAM);
    if (!mesh->positions || !mesh->uvs || !mesh->indices) {
        if (mesh->positions) vglFree(mesh->positions);
        if (mesh->uvs) vglFree(mesh->uvs);
        if (mesh->indices) vglFree(mesh->indices);
        free(mesh);
        return NULL;
    }
    for (uint32_t i = 0; i < vertex_count; ++i) {
        memcpy((float *)mesh->positions + i * 3, vertices[i].position, 3 * sizeof(float));
        memcpy((float *)mesh->uvs + i * 2, vertices[i].uv, 2 * sizeof(float));
    }
    memcpy(mesh->indices, indices, index_count * sizeof(uint16_t));
    mesh->count = index_count;
    return mesh;
}

void cbn_begin(const float *projection, const float *view) {
    glClearColor(0.35f, 0.58f, 0.85f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
    glViewport(0, 0, CBN_WIDTH, CBN_HEIGHT);
    glMatrixMode(GL_PROJECTION);
    glLoadMatrixf(projection);
    glMatrixMode(GL_MODELVIEW);
    glLoadMatrixf(view);
}

void cbn_draw(const void *handle, float x, float y, float z, const float *color, uint32_t texture) {
    const CbnMesh *mesh = handle;
    glPushMatrix();
    glTranslatef(x, y, z);
    glColor4fv(color);
    if (texture) {
        glEnable(GL_TEXTURE_2D);
        glEnableClientState(GL_TEXTURE_COORD_ARRAY);
        glBindTexture(GL_TEXTURE_2D, texture);
        vglTexCoordPointerMapped(mesh->uvs);
    } else {
        glDisable(GL_TEXTURE_2D);
        glDisableClientState(GL_TEXTURE_COORD_ARRAY);
    }
    vglVertexPointerMapped(3, mesh->positions);
    vglIndexPointerMapped(mesh->indices);
    vglDrawObjects(GL_TRIANGLES, mesh->count);
    glPopMatrix();
}

void cbn_present(void) { vglSwapBuffers(GL_FALSE); }

/* Developer aiming aid. The game HUD must eventually use its shared UI output. */
void cbn_developer_reticle(void) {
    glEnable(GL_SCISSOR_TEST);
    glClearColor(1, 1, 1, 1);
    glScissor(CBN_WIDTH / 2 - 6, CBN_HEIGHT / 2 - 1, 12, 2);
    glClear(GL_COLOR_BUFFER_BIT);
    glScissor(CBN_WIDTH / 2 - 1, CBN_HEIGHT / 2 - 6, 2, 12);
    glClear(GL_COLOR_BUFFER_BIT);
    glDisable(GL_SCISSOR_TEST);
}

void cbn_dimensions(uint32_t *size) {
    size[0] = CBN_WIDTH;
    size[1] = CBN_HEIGHT;
}

/* Explicit readback for developer frame inspection, never part of the frame loop. */
int cbn_read_pixels(uint8_t *rgba, uint32_t capacity) {
    if (capacity != CBN_WIDTH * CBN_HEIGHT * 4) return 0;
    glFinish();
    if (sceGxmDisplayQueueFinish() < 0) return 0;
    SceDisplayFrameBuf frame = { .size = sizeof(frame) };
    if (sceDisplayGetFrameBuf(&frame, SCE_DISPLAY_SETBUF_NEXTFRAME) < 0 ||
        !frame.base || frame.width != CBN_WIDTH || frame.height != CBN_HEIGHT ||
        frame.pixelformat != SCE_DISPLAY_PIXELFORMAT_A8B8G8R8) return 0;
    /* Use a positive-stride transfer from the exact surface base. VitaGL's
       direct CPU read can be stale under emulation; its negative-stride transfer
       starts inside the surface, which Vita3K's exact-address lookup misses.
       Rust admits this temporary mapped allocation before calling us. */
    uint8_t *staging = vglAlloc(capacity, VGL_MEM_RAM);
    if (!staging) return 0;
    int copied = sceGxmTransferCopy(CBN_WIDTH, CBN_HEIGHT, 0, 0,
        SCE_GXM_TRANSFER_COLORKEY_NONE,
        SCE_GXM_TRANSFER_FORMAT_U8U8U8U8_ABGR, SCE_GXM_TRANSFER_LINEAR,
        frame.base, 0, 0, frame.pitch * 4,
        SCE_GXM_TRANSFER_FORMAT_U8U8U8U8_ABGR, SCE_GXM_TRANSFER_LINEAR,
        staging, 0, 0, CBN_WIDTH * 4, NULL, 0, NULL);
    if (copied >= 0) {
        /* Completion precedes both CPU inspection and mapped-buffer release. */
        /* If completion is unknown, retain the mapped storage until process exit.
           The negative status also retains its Rust admission lease. */
        if (sceGxmTransferFinish() < 0) return -1;
        for (uint32_t y = 0; y < CBN_HEIGHT; ++y) {
            memcpy(rgba + y * CBN_WIDTH * 4,
                staging + (CBN_HEIGHT - y - 1) * CBN_WIDTH * 4, CBN_WIDTH * 4);
        }
    }
    vglFree(staging);
    return copied >= 0;
}

/* Conservative completion for initial ownership correctness. A nonblocking
   GXM retirement queue is required before streaming performance acceptance. */
void cbn_release(void *handle) {
    CbnMesh *mesh = handle;
    glFinish();
    vglFree(mesh->positions);
    vglFree(mesh->uvs);
    vglFree(mesh->indices);
    free(mesh);
}

/* vitaGL has no teardown API in this pinned package. The context is process-owned;
   do not attempt a second initialization. The OS releases it on process exit. */
void cbn_shutdown(void) { glFinish(); }
