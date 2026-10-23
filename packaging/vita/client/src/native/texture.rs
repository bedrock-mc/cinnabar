use super::{Lease, NativeError, PhantomData, Pool, Renderer};
use crate::texture::TextureLayer;
use std::num::NonZeroU32;

unsafe extern "C" {
    fn cbn_texture_upload(size: u32, pixels: *const u8, bytes: u32) -> u32;
    fn cbn_texture_release(texture: u32);
}

pub struct GpuTexture<'context> {
    id: NonZeroU32,
    _main: Lease,
    _cdram: Lease,
    _context: PhantomData<&'context Renderer>,
}

impl Renderer {
    pub fn upload_texture(&self, source: &TextureLayer<'_>) -> Result<GpuTexture<'_>, NativeError> {
        let bytes = u32::try_from(source.bytes()).map_err(|_| NativeError::Texture)?;
        // VitaGL may choose either pool for this immutable base image. Reserve
        // in both conservatively until explicit texture-pool placement is wired.
        let main = self
            .budget
            .reserve(Pool::Main, source.bytes())
            .map_err(NativeError::Pressure)?;
        let cdram = self
            .budget
            .reserve(Pool::Cdram, source.bytes())
            .map_err(NativeError::Pressure)?;
        // SAFETY: validated layer size and contiguous RGBA remain live until the
        // synchronous copy completes. Native code also checks its hardware limit.
        let id = NonZeroU32::new(unsafe {
            cbn_texture_upload(source.size(), source.pixels.as_ptr(), bytes)
        })
        .ok_or(NativeError::Texture)?;
        Ok(GpuTexture {
            id,
            _main: main,
            _cdram: cdram,
            _context: PhantomData,
        })
    }
}

impl GpuTexture<'_> {
    pub(super) fn id(&self) -> u32 {
        self.id.get()
    }
}

impl Drop for GpuTexture<'_> {
    fn drop(&mut self) {
        // SAFETY: unique ownership, creating thread, process-owned context alive.
        // The native release completes GPU work before returning both admissions.
        unsafe {
            cbn_texture_release(self.id.get());
        }
    }
}
