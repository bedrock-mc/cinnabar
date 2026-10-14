use super::*;

#[test]
fn every_pattern_position_uses_the_bare_identifier_context() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for source in [
        format!("fn f() {{ if let {bare:?} = name {{}} }}"),
        format!("fn f() {{ while let {bare:?} = name {{ break; }} }}"),
        format!("fn f() {{ let Some({bare:?}) = name else {{ return; }}; }}"),
        format!("fn f() {{ let ({bare:?}, _) = name; }}"),
        format!("fn f(Some({bare:?}): Option<&str>) {{}}"),
        format!("fn f() {{ let _ = |Some({bare:?})| (); }}"),
        format!("fn f() {{ for Some({bare:?}) in names {{}} }}"),
        format!("fn f() {{ if let [{bare:?}, ..] = names {{}} }}"),
        format!("fn f() {{ if let S {{ name: {bare:?} }} = value {{}} }}"),
        format!("fn f() {{ if let pattern!({bare:?}) = name {{}} }}"),
    ] {
        assert_eq!(literals(&source, &vocabulary), [bare], "{source}");
    }
}

#[test]
fn pattern_context_does_not_extend_into_initializers_or_closure_bodies() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for source in [
        format!("fn f() {{ let name = {bare:?}; }}"),
        format!("fn f() {{ let Some(name) = {bare:?} else {{ return; }}; }}"),
        format!("fn f() {{ if let Some(name) = {bare:?} {{}} }}"),
        format!("fn f() {{ while let Some(name) = {bare:?} {{ break; }} }}"),
        format!("fn f() {{ for name in display({bare:?}) {{}} }}"),
        format!("fn f() {{ let _ = |name| display({bare:?}); }}"),
    ] {
        assert!(literals(&source, &vocabulary).is_empty(), "{source}");
    }
    let source = format!("fn f() {{ let Some({bare:?}) = {bare:?} else {{ return; }}; }}");
    assert_eq!(literals(&source, &vocabulary), [bare]);
}
