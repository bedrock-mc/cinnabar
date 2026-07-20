use assets::{LightProperties, read_light_registry};
use sha2::{Digest, Sha256};

fn lreg(breg: &[u8], properties: &[LightProperties]) -> Vec<u8> {
    let mut bytes = b"LREG1001".to_vec();
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&(properties.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    bytes.extend(properties.iter().map(|light| light.packed()));
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

#[test]
fn lreg1001_decodes_exactly_one_byte_per_breg_state() {
    let breg = b"exact BREG1003 bytes";
    let expected = [
        LightProperties::new(0, 15).unwrap(),
        LightProperties::new(13, 2).unwrap(),
    ];
    let decoded = read_light_registry(&lreg(breg, &expected), breg, expected.len())
        .expect("decode bound LREG1001");
    assert_eq!(decoded.as_ref(), expected);
    assert!(
        decoded
            .iter()
            .all(|light| light.emission() <= 15 && light.filter() <= 15)
    );
}

#[test]
fn lreg1001_rejects_breg_hash_and_count_mismatch() {
    let breg = b"exact BREG1003 bytes";
    let bytes = lreg(breg, &[LightProperties::new(1, 2).unwrap()]);
    assert!(read_light_registry(&bytes, b"different BREG1003 bytes", 1).is_err());
    assert!(read_light_registry(&bytes, breg, 2).is_err());
}

#[test]
fn lreg1001_rejects_malformed_codec_and_integrity() {
    let breg = b"exact BREG1003 bytes";
    let valid = lreg(breg, &[LightProperties::new(1, 2).unwrap()]);
    let mutations: [fn(&mut Vec<u8>); 5] = [
        |bytes: &mut Vec<u8>| bytes[0] ^= 1,
        |bytes: &mut Vec<u8>| bytes[8] ^= 1,
        |bytes: &mut Vec<u8>| bytes[48] ^= 1,
        |bytes: &mut Vec<u8>| bytes.push(0),
        |bytes: &mut Vec<u8>| {
            bytes.truncate(bytes.len() - 1);
        },
    ];
    for mutation in mutations {
        let mut malformed = valid.clone();
        mutation(&mut malformed);
        assert!(read_light_registry(&malformed, breg, 1).is_err());
    }
}

#[test]
fn light_properties_rejects_malformed_runtime_accessor_values() {
    assert!(LightProperties::new(16, 0).is_err());
    assert!(LightProperties::new(0, 16).is_err());
}
