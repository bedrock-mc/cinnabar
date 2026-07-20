pub const TILE_SIZE: u32 = 16;
pub const MIP_COUNT: u32 = 5;

/// One mip level containing every array layer in layer-major RGBA8 order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureMip {
    pub size: u32,
    pub rgba8: Box<[u8]>,
}

/// Equal-sized 16x16 texture-array layers with independent mip chains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureArray {
    pub layers: u32,
    pub mips: Box<[TextureMip]>,
}
