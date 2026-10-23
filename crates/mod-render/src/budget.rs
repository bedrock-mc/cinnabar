/// One bundle's GPU allowance, shared by every remapped resource owner it produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuReservation {
    pub owners: Vec<u64>,
    pub geometry_bytes: u64,
    pub limit_bytes: u64,
}
