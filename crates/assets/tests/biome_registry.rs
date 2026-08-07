use assets::read_biome_registry;
use sha2::{Digest, Sha256};

const REGISTRY: &[u8] = include_bytes!("../data/biome-registry-v1001.bin");

#[test]
fn checked_in_biome_registry_is_the_exact_retail_projection() {
    let records = read_biome_registry(REGISTRY).expect("decode checked-in biome registry");
    assert_eq!(records.len(), 87);
    assert!(records.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(records.iter().all(|record| record.id != 194));
    assert_eq!(
        format!("{:x}", Sha256::digest(REGISTRY)),
        "1c5c567c38bad94f61f21b83f2848db151fcd07a44a3bdcc2aea0c8ae5f9b62c"
    );

    let mut with_trailing_byte = REGISTRY.to_vec();
    with_trailing_byte.push(0);
    assert!(read_biome_registry(&with_trailing_byte).is_err());
}
