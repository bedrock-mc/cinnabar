//! Main-thread VitaGL ownership. Mesh lifetimes cannot outlive their context.

use crate::{
    controls::PadSample,
    memory::{Budget, DEFAULT_CDRAM_LIMIT, Lease, MIB, Pool, Pressure},
    terrain::{CubeUpload, Vertex},
};
use std::{
    ffi::c_void,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
mod texture;
pub use texture::GpuTexture;
const GRAPHICS_MAIN_BYTES: u32 = 48 * MIB as u32;
const GRAPHICS_CDRAM_BYTES: u32 = DEFAULT_CDRAM_LIMIT as u32;

unsafe extern "C" {
    fn cbn_init(main: u32, cdram: u32) -> i32;
    fn cbn_time_us() -> u64;
    fn cbn_poll(pad: *mut PadSample) -> i32;
    fn cbn_upload(
        vertices: *const Vertex,
        vertex_count: u32,
        indices: *const u16,
        index_count: u32,
    ) -> *mut c_void;
    fn cbn_begin(projection: *const f32, view: *const f32);
    fn cbn_draw(mesh: *const c_void, x: f32, y: f32, z: f32, color: *const f32, texture: u32);
    fn cbn_present();
    fn cbn_developer_reticle();
    fn cbn_dimensions(size: *mut u32);
    fn cbn_read_pixels(rgba: *mut u8, capacity: u32) -> i32;
    fn cbn_release(mesh: *mut c_void);
    fn cbn_shutdown();
}

#[derive(Debug)]
pub enum NativeError {
    AlreadyActive,
    Initialization,
    EmptyMesh,
    Allocation,
    Readback,
    Texture,
    Pressure(Pressure),
}

pub struct Renderer {
    budget: Budget,
    _main_thread: PhantomData<Rc<()>>,
}

impl Renderer {
    pub fn new(budget: Budget) -> Result<Self, NativeError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| NativeError::AlreadyActive)?;
        // SAFETY: this guard provides unique process ownership; no pointer arguments.
        if unsafe { cbn_init(GRAPHICS_MAIN_BYTES, GRAPHICS_CDRAM_BYTES) } == 0 {
            // Failed initialization may have allocated partial GXM state. Do not retry
            // within the same process without a verified partial-init cleanup contract.
            return Err(NativeError::Initialization);
        }
        Ok(Self {
            budget,
            _main_thread: PhantomData,
        })
    }

    pub fn time_us(&self) -> u64 {
        // SAFETY: no pointer arguments; SDK monotonic process clock.
        unsafe { cbn_time_us() }
    }

    pub fn poll(&self) -> Option<PadSample> {
        let mut pad = PadSample::default();
        // SAFETY: repr(C) pad has compile-time matching size and a live unique pointer.
        (unsafe { cbn_poll(&mut pad) } != 0).then_some(pad)
    }

    pub fn upload(&self, upload: &CubeUpload) -> Result<GpuMesh<'_>, NativeError> {
        if upload.indices().is_empty() {
            return Err(NativeError::EmptyMesh);
        }
        let lease = self
            .budget
            .reserve(Pool::Main, upload.bytes())
            .map_err(NativeError::Pressure)?;
        // SAFETY: slices are live for the synchronous copy; CubeUpload bounds all
        // sizes and indices. The returned native allocation has one owner.
        let pointer = unsafe {
            cbn_upload(
                upload.vertices().as_ptr(),
                upload.vertices().len() as u32,
                upload.indices().as_ptr(),
                upload.indices().len() as u32,
            )
        };
        let pointer = NonNull::new(pointer).ok_or(NativeError::Allocation)?;
        Ok(GpuMesh {
            pointer,
            _lease: lease,
            _context: PhantomData,
        })
    }

    pub fn begin(&self, projection: &[f32; 16], view: &[f32; 16]) {
        // SAFETY: both matrices remain live for the synchronous copy.
        unsafe {
            cbn_begin(projection.as_ptr(), view.as_ptr());
        }
    }
    pub fn present(&self) {
        // SAFETY: renderer owns a live context and is confined to its creating thread.
        unsafe {
            cbn_present();
        }
    }

    /// A diagnostic aiming aid, not the Minecraft HUD implementation.
    pub fn developer_reticle(&self) {
        // SAFETY: no pointer arguments; this renderer owns the current context.
        unsafe {
            cbn_developer_reticle();
        }
    }

    pub fn dimensions(&self) -> [u32; 2] {
        let mut size = [0; 2];
        // SAFETY: the native function writes exactly two u32 values.
        unsafe {
            cbn_dimensions(size.as_mut_ptr());
        }
        size
    }

    /// Read the most recently presented frame into caller-owned RGBA storage. Intended for
    /// frame inspection, not streaming; it stalls until the GPU completes.
    pub fn read_pixels(&self, rgba: &mut [u8]) -> Result<(), NativeError> {
        let capacity = u32::try_from(rgba.len()).map_err(|_| NativeError::Readback)?;
        let staging = self
            .budget
            .reserve(Pool::Main, rgba.len())
            .map_err(NativeError::Pressure)?;
        // SAFETY: native code checks capacity against its fixed framebuffer size
        // before writing; the mutable slice remains live for the entire call.
        let status = unsafe { cbn_read_pixels(rgba.as_mut_ptr(), capacity) };
        if status < 0 {
            // The native transfer's completion is unknown. Its mapped storage is
            // quarantined until process exit, so its admission must stay reserved.
            std::mem::forget(staging);
        }
        if status <= 0 {
            return Err(NativeError::Readback);
        }
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // SAFETY: all borrowed GpuMesh objects have already been dropped.
        unsafe {
            cbn_shutdown();
        }
        // vitaGL has no teardown API. Keep ACTIVE set: context pools belong to
        // this process until exit, and a second initialization would leak them.
    }
}

pub struct GpuMesh<'context> {
    pointer: NonNull<c_void>,
    _lease: Lease,
    _context: PhantomData<&'context Renderer>,
}

impl<'context> GpuMesh<'context> {
    pub fn draw(&self, origin: [f32; 3], color: [f32; 4]) {
        // SAFETY: native ownership is live, with a live context; color is copied.
        unsafe {
            cbn_draw(
                self.pointer.as_ptr(),
                origin[0],
                origin[1],
                origin[2],
                color.as_ptr(),
                0,
            );
        }
    }

    pub fn draw_textured(&self, origin: [f32; 3], color: [f32; 4], texture: &GpuTexture<'context>) {
        // SAFETY: both resources borrow the unique process-owned renderer. The
        // texture remains owned through the draw and waits for completion on drop.
        unsafe {
            cbn_draw(
                self.pointer.as_ptr(),
                origin[0],
                origin[1],
                origin[2],
                color.as_ptr(),
                texture.id(),
            );
        }
    }
}

impl Drop for GpuMesh<'_> {
    fn drop(&mut self) {
        // SAFETY: waits for GPU completion before freeing; lease drops afterwards.
        unsafe {
            cbn_release(self.pointer.as_ptr());
        }
    }
}
