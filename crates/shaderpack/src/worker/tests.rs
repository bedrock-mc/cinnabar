use super::*;

#[test]
fn payload_limits_reject_extra_bytes_before_deserialization() {
    assert_eq!(read_limited(b"abc".as_slice(), 3).unwrap(), b"abc");
    assert_eq!(
        read_limited(b"abcd".as_slice(), 3).unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
    let mut output = Vec::new();
    let mut writer = LimitedWriter {
        writer: &mut output,
        remaining: 3,
    };
    writer.write_all(b"ab").unwrap();
    assert_eq!(
        writer.write_all(b"cd").unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
    assert_eq!(output, b"ab");
}
