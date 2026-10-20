//! Queue writes with caller attribution; wrappers inline to the original API in normal builds.

use render_scene::runtime_profile_uploads::{UploadKind, UploadPath, record, record_at};

/// Queue upload entry points that retain the caller's source location.
pub(crate) trait ProfiledQueue {
    /// Queues buffer bytes unchanged and counts the request at the caller.
    #[track_caller]
    fn profiled_write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]);

    /// Preserves the originating owner when a helper forwards fallback bytes.
    fn profiled_write_buffer_at(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        data: &[u8],
        site: &'static std::panic::Location<'static>,
    );

    /// Retains the owner label when a helper coalesces fallback bytes into a queue view.
    fn profiled_write_buffer_with_at(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        size: wgpu::BufferSize,
        site: &'static std::panic::Location<'static>,
    ) -> Option<wgpu::QueueWriteBufferView>;

    /// Queues the original texture payload; bytes include any caller-supplied row padding.
    #[track_caller]
    fn profiled_write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    );
}

impl ProfiledQueue for wgpu::Queue {
    #[inline]
    #[track_caller]
    fn profiled_write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        self.profiled_write_buffer_at(buffer, offset, data, std::panic::Location::caller());
    }

    #[inline]
    fn profiled_write_buffer_at(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        data: &[u8],
        site: &'static std::panic::Location<'static>,
    ) {
        self.write_buffer(buffer, offset, data);
        record_at(
            site,
            UploadKind::Buffer,
            UploadPath::Queue,
            data.len() as u64,
        );
    }

    #[inline]
    fn profiled_write_buffer_with_at(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        size: wgpu::BufferSize,
        site: &'static std::panic::Location<'static>,
    ) -> Option<wgpu::QueueWriteBufferView> {
        let view = self.write_buffer_with(buffer, offset, size);
        if view.is_some() {
            record_at(site, UploadKind::Buffer, UploadPath::Queue, size.get());
        }
        view
    }

    #[inline]
    #[track_caller]
    fn profiled_write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        self.write_texture(texture, data, layout, size);
        record(UploadKind::Texture, UploadPath::Queue, data.len() as u64);
    }
}
