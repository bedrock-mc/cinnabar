use render_model::ui_page_uniform::*;

#[test]
fn repeat_page_table_matches_the_shader_uniform_layout() {
    let text = super::shader::source(include_str!("../ui.wgsl"));
    let module = naga::front::wgsl::parse_str(&text).expect("UI shader parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("UI shader validates");
    let (_, layout) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("PageFormat"))
        .unwrap();
    let naga::TypeInner::Struct { members, span } = &layout.inner else {
        panic!("page format is a uniform struct");
    };
    assert_eq!(u64::from(*span), BYTE_SIZE);
    assert_eq!(u64::from(members[1].offset), offset(0));
    assert_eq!(words(false).len() * size_of::<u32>(), BYTE_SIZE as usize);
}
