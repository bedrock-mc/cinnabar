use super::*;

#[test]
fn relative_module_roots_do_not_depend_on_glob_conditions() {
    let module = vec!["crate".into(), "parent".into(), "child".into()];
    let mut symbols = Symbols::default();
    for depth in 1..module.len() {
        let parent = module[..depth].to_vec();
        symbols.globs.insert(
            parent.clone(),
            vec![Import {
                condition: Condition::Atom("feature=unrelated".into()),
                module: parent.clone(),
                target: vec!["other".into()],
                scope: parent,
            }],
        );
    }
    symbols
        .modules
        .insert(module[..2].to_vec(), vec![Condition::Always]);
    for (path, depth) in [
        (vec!["self"], 3),
        (vec!["super"], 2),
        (vec!["self", "super"], 2),
        (vec!["super", "super"], 1),
    ] {
        let path = path.into_iter().map(String::from).collect::<Vec<_>>();
        assert_eq!(
            resolve(
                &module,
                &path,
                &symbols,
                &mut BTreeSet::new(),
                &Configuration::default(),
                false,
            ),
            Ok(module[..depth].to_vec()),
            "{} must name its containing module without inspecting imports",
            path.join("::"),
        );
    }
}
