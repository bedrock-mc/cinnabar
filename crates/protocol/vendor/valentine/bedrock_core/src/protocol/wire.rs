use bytes::{Buf, BufMut};

#[inline]
pub fn write_var_u32<B: BufMut>(buf: &mut B, mut v: u32) {
    while v >= 0x80 {
        buf.put_u8((v as u8) | 0x80);
        v >>= 7;
    }
    buf.put_u8(v as u8);
}

#[inline]
pub fn var_u32_len(mut v: u32) -> usize {
    let mut len = 1;
    while v >= 0x80 {
        v >>= 7;
        len += 1;
    }
    len
}

#[inline]
pub fn read_var_u32<B: Buf>(buf: &mut B) -> Result<u32, std::io::Error> {
    let mut result: u32 = 0;
    let mut shift = 0u32;
    loop {
        if !buf.has_remaining() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "varu32 eof",
            ));
        }
        let byte = buf.get_u8();
        result |= ((byte & 0x7F) as u32) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "varu32 too long",
            ));
        }
    }
    Ok(result)
}

#[inline]
pub fn write_var_u64<B: BufMut>(buf: &mut B, mut v: u64) {
    while v >= 0x80 {
        buf.put_u8((v as u8) | 0x80);
        v >>= 7;
    }
    buf.put_u8(v as u8);
}

#[inline]
pub fn var_u64_len(mut v: u64) -> usize {
    let mut len = 1;
    while v >= 0x80 {
        v >>= 7;
        len += 1;
    }
    len
}

#[inline]
pub fn read_var_u64<B: Buf>(buf: &mut B) -> Result<u64, std::io::Error> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if !buf.has_remaining() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "varu64 eof",
            ));
        }
        let byte = buf.get_u8();
        result |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "varu64 too long",
            ));
        }
    }
    Ok(result)
}

#[inline]
pub fn zigzag32_encode(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

#[inline]
pub fn zigzag32_decode(v: u32) -> i32 {
    ((v >> 1) as i32) ^ (-((v & 1) as i32))
}

#[inline]
pub fn zigzag64_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
pub fn zigzag64_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

#[inline]
pub fn write_zigzag32<B: BufMut>(buf: &mut B, v: i32) {
    write_var_u32(buf, zigzag32_encode(v));
}

#[inline]
pub fn read_zigzag32<B: Buf>(buf: &mut B) -> Result<i32, std::io::Error> {
    Ok(zigzag32_decode(read_var_u32(buf)?))
}

#[inline]
pub fn write_zigzag64<B: BufMut>(buf: &mut B, v: i64) {
    write_var_u64(buf, zigzag64_encode(v));
}

#[inline]
pub fn read_zigzag64<B: Buf>(buf: &mut B) -> Result<i64, std::io::Error> {
    Ok(zigzag64_decode(read_var_u64(buf)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    // ========== VarU32 Tests ==========

    #[test]
    fn varu32_zero_single_byte() {
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, 0);
        assert_eq!(buf.as_ref(), &[0x00]);

        let mut reader = buf.freeze();
        let decoded = read_var_u32(&mut reader).unwrap();
        assert_eq!(decoded, 0);
    }

    #[test]
    fn varu32_single_byte_boundary() {
        // 127 is max single-byte value (0x7F)
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, 127);
        assert_eq!(buf.as_ref(), &[0x7F]);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u32(&mut reader).unwrap(), 127);
    }

    #[test]
    fn varu32_two_byte_minimum() {
        // 128 requires two bytes
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, 128);
        assert_eq!(buf.as_ref(), &[0x80, 0x01]);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u32(&mut reader).unwrap(), 128);
    }

    #[test]
    fn varu32_two_byte_maximum() {
        // 16383 is max two-byte value
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, 16383);
        assert_eq!(buf.as_ref(), &[0xFF, 0x7F]);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u32(&mut reader).unwrap(), 16383);
    }

    #[test]
    fn varu32_three_byte_minimum() {
        // 16384 requires three bytes
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, 16384);
        assert_eq!(buf.as_ref(), &[0x80, 0x80, 0x01]);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u32(&mut reader).unwrap(), 16384);
    }

    #[test]
    fn varu32_max_value() {
        let mut buf = BytesMut::new();
        write_var_u32(&mut buf, u32::MAX);
        // u32::MAX = 4294967295 requires 5 bytes
        assert_eq!(buf.len(), 5);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u32(&mut reader).unwrap(), u32::MAX);
    }

    #[test]
    fn varu32_roundtrip_samples() {
        for value in [
            0,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            2097151,
            2097152,
            u32::MAX,
        ] {
            let mut buf = BytesMut::new();
            write_var_u32(&mut buf, value);
            let mut reader = buf.freeze();
            assert_eq!(
                read_var_u32(&mut reader).unwrap(),
                value,
                "roundtrip failed for {}",
                value
            );
        }
    }

    #[test]
    fn varu32_empty_buffer_error() {
        let mut reader = &[][..];
        let err = read_var_u32(&mut reader).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn varu32_too_long_error() {
        // 6 bytes with continuation bits set
        let mut reader = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01][..];
        let err = read_var_u32(&mut reader).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    // ========== VarU64 Tests ==========

    #[test]
    fn varu64_zero_single_byte() {
        let mut buf = BytesMut::new();
        write_var_u64(&mut buf, 0);
        assert_eq!(buf.as_ref(), &[0x00]);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u64(&mut reader).unwrap(), 0);
    }

    #[test]
    fn varu64_max_value() {
        let mut buf = BytesMut::new();
        write_var_u64(&mut buf, u64::MAX);
        // u64::MAX requires 10 bytes
        assert_eq!(buf.len(), 10);

        let mut reader = buf.freeze();
        assert_eq!(read_var_u64(&mut reader).unwrap(), u64::MAX);
    }

    #[test]
    fn varu64_roundtrip_samples() {
        for value in [0, 1, 127, 128, u32::MAX as u64, u64::MAX / 2, u64::MAX] {
            let mut buf = BytesMut::new();
            write_var_u64(&mut buf, value);
            let mut reader = buf.freeze();
            assert_eq!(
                read_var_u64(&mut reader).unwrap(),
                value,
                "roundtrip failed for {}",
                value
            );
        }
    }

    #[test]
    fn varu64_empty_buffer_error() {
        let mut reader = &[][..];
        let err = read_var_u64(&mut reader).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn varu64_too_long_error() {
        // 11 bytes with continuation bits set
        let data = [0x80; 11];
        let mut reader = &data[..];
        let err = read_var_u64(&mut reader).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    // ========== ZigZag32 Tests ==========

    #[test]
    fn zigzag32_zero_encodes_to_zero() {
        assert_eq!(zigzag32_encode(0), 0);
        assert_eq!(zigzag32_decode(0), 0);
    }

    #[test]
    fn zigzag32_minus_one_encodes_to_one() {
        assert_eq!(zigzag32_encode(-1), 1);
        assert_eq!(zigzag32_decode(1), -1);
    }

    #[test]
    fn zigzag32_one_encodes_to_two() {
        assert_eq!(zigzag32_encode(1), 2);
        assert_eq!(zigzag32_decode(2), 1);
    }

    #[test]
    fn zigzag32_pattern_verification() {
        // ZigZag pattern: 0 -> 0, -1 -> 1, 1 -> 2, -2 -> 3, 2 -> 4, ...
        let pairs = [(0, 0), (-1, 1), (1, 2), (-2, 3), (2, 4), (-3, 5), (3, 6)];
        for (signed, unsigned) in pairs {
            assert_eq!(
                zigzag32_encode(signed),
                unsigned,
                "encode {} -> {}",
                signed,
                unsigned
            );
            assert_eq!(
                zigzag32_decode(unsigned),
                signed,
                "decode {} -> {}",
                unsigned,
                signed
            );
        }
    }

    #[test]
    fn zigzag32_extremes() {
        // i32::MAX encodes to u32::MAX - 1
        assert_eq!(zigzag32_encode(i32::MAX), u32::MAX - 1);
        assert_eq!(zigzag32_decode(u32::MAX - 1), i32::MAX);

        // i32::MIN encodes to u32::MAX
        assert_eq!(zigzag32_encode(i32::MIN), u32::MAX);
        assert_eq!(zigzag32_decode(u32::MAX), i32::MIN);
    }

    #[test]
    fn zigzag32_wire_roundtrip() {
        for value in [0, 1, -1, 127, -128, i32::MAX, i32::MIN] {
            let mut buf = BytesMut::new();
            write_zigzag32(&mut buf, value);
            let mut reader = buf.freeze();
            assert_eq!(
                read_zigzag32(&mut reader).unwrap(),
                value,
                "roundtrip failed for {}",
                value
            );
        }
    }

    // ========== ZigZag64 Tests ==========

    #[test]
    fn zigzag64_zero_encodes_to_zero() {
        assert_eq!(zigzag64_encode(0), 0);
        assert_eq!(zigzag64_decode(0), 0);
    }

    #[test]
    fn zigzag64_minus_one_encodes_to_one() {
        assert_eq!(zigzag64_encode(-1), 1);
        assert_eq!(zigzag64_decode(1), -1);
    }

    #[test]
    fn zigzag64_extremes() {
        assert_eq!(zigzag64_encode(i64::MAX), u64::MAX - 1);
        assert_eq!(zigzag64_decode(u64::MAX - 1), i64::MAX);

        assert_eq!(zigzag64_encode(i64::MIN), u64::MAX);
        assert_eq!(zigzag64_decode(u64::MAX), i64::MIN);
    }

    #[test]
    fn zigzag64_wire_roundtrip() {
        for value in [0, 1, -1, 127, -128, i64::MAX, i64::MIN] {
            let mut buf = BytesMut::new();
            write_zigzag64(&mut buf, value);
            let mut reader = buf.freeze();
            assert_eq!(
                read_zigzag64(&mut reader).unwrap(),
                value,
                "roundtrip failed for {}",
                value
            );
        }
    }
}
