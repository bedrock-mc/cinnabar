use bytemuck::{Pod, Zeroable};

/// Whether one stored triangle also represents the cube's opposing authored face.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
pub struct ActorRigSurface(u32);

impl ActorRigSurface {
    pub const SINGLE_FACE: Self = Self(0);
    pub const OPPOSING_FACES: Self = Self(1);

    pub(super) const fn is_valid(self) -> bool {
        self.0 <= Self::OPPOSING_FACES.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_contract_has_only_single_and_collapsed_opposing_faces() {
        assert!(ActorRigSurface::SINGLE_FACE.is_valid());
        assert!(ActorRigSurface::OPPOSING_FACES.is_valid());
        let unknown: ActorRigSurface = bytemuck::pod_read_unaligned(&2_u32.to_le_bytes());
        assert!(!unknown.is_valid());
    }
}
