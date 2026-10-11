//! Optional pack sky textures, independent of the required atmosphere carrier.

/// Numbered faces in the pack's authored order.
pub const SKYBOX_FACE_COUNT: usize = 6;
/// Fresh settings preserve Bedrock’s ordinary skybox translation.
pub const DEFAULT_NATIVE_SKYBOX_OFFSET: bool = true;

/// Dimensions with distinct environment texture namespaces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum SkyboxDimension {
    Overworld,
    Nether,
    End,
}

impl SkyboxDimension {
    pub const ALL: [Self; 3] = [Self::Overworld, Self::Nether, Self::End];

    /// Texture prefix used by the dimension's numbered faces.
    pub const fn texture_prefix(self) -> &'static str {
        match self {
            Self::Overworld => "textures/environment/overworld_cubemap/cubemap_",
            Self::Nether => "textures/environment/nether_cubemap/cubemap_",
            Self::End => "textures/environment/theend_cubemap/cubemap_",
        }
    }
}

/// A decoded straight-alpha image; optional faces retain the ordinary sky beneath them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkyboxFace {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// A dimension's numbered images; absent or invalid faces use the stock sky.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackSkybox {
    pub faces: [Option<SkyboxFace>; SKYBOX_FACE_COUNT],
}

impl PackSkybox {
    /// Encodes exactly the faces that can contribute to the background pass.
    pub fn face_mask(&self) -> u32 {
        self.faces
            .iter()
            .enumerate()
            .fold(0, |mask, (face, image)| {
                mask | ((image.is_some() as u32) << face)
            })
    }
}
