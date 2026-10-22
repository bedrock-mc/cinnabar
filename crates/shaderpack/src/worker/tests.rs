use super::*;

#[test]
fn accepted_expression_depths_survive_the_compiler_payload() {
    let count = crate::expr::MAX_TREE_DEPTH as usize;
    let sum = std::iter::repeat_n("1", count)
        .collect::<Vec<_>>()
        .join("+");
    let pack = ShaderPack::new(crate::PackSource::from_files([
        (
            "final.vsh",
            "#version 330\nvoid main() { gl_Position=vec4(0.0); }".into(),
        ),
        (
            "final.fsh",
            "#version 330\nout vec4 color; void main() { color=vec4(1.0); }".into(),
        ),
        (
            "shaders.properties",
            format!("program.final.enabled={sum}\nuniform.float.level={sum}\n"),
        ),
    ]));
    let config = TranslateConfig::default();
    let translation = pack.translate(&config);
    assert_eq!(translation.properties.program_rules.len(), 1);
    let compiled = prepare_translation(pack, config, translation);
    assert!(compiled.custom_uniforms.1.is_empty());
    let result = Ok::<_, String>(Response::Render(Box::new(compiled)));
    let mut bytes = Vec::new();
    ciborium::into_writer(&result, &mut bytes).unwrap();
    let mut decoder = Decoder::start(std::io::Cursor::new(bytes)).unwrap();
    let mut result = None;
    test_time::eventually("accepted expression payload decoded", || {
        result = decoder.poll();
        result.is_some()
    });
    let mut decoded = result.unwrap().unwrap().into_translation().unwrap();
    assert!(decoded.translation.properties.program_rules[0].is_enabled(&Default::default()));
    assert_eq!(
        decoded.custom_uniforms.0.evaluate(&|_| None, 0.0),
        [("level".into(), crate::UniformValue::Float(count as f32))]
    );
}

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
