use super::*;

const WASM: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn manifest(body: &str) -> String {
    format!(
        "id = \"bei\"\nversion = \"0.1.0\"\napi = \"{API}\"\n\
         permissions = [\"screen\", \"items\", \"recipes\", \"keys\"]\n{body}"
    )
}

fn files(paths: &[&str]) -> String {
    let mut out = format!("[files]\n\"mod.wasm\" = \"{WASM}\"\n");
    for path in paths {
        out.push_str(&format!("\"{path}\" = \"{WASM}\"\n"));
    }
    out
}

fn refusal(text: &str) -> String {
    ModManifest::parse(text).unwrap_err()
}

#[test]
fn a_complete_package_parses() {
    let text = manifest(&format!(
        "templates = [\"ui/overlay.json\"]\ntextures = [\"textures/arrow.png\"]\n\
         actions = [\"bei.next_page\"]\n\
         [[keys]]\nid = \"bei.show_recipes\"\nkey = \"r\"\nlabel = \"key.bei.show_recipes\"\n\
         [[keys]]\nid = \"bei.toggle\"\nkey = \"o\"\nmodifiers = [\"ctrl\"]\nlabel = \"key.bei.toggle\"\n{}",
        files(&["ui/overlay.json", "textures/arrow.png"])
    ));
    let manifest = ModManifest::parse(&text).unwrap();
    assert_eq!(manifest.id, "bei");
    assert_eq!(manifest.namespace(), "bei");
    assert!(manifest.permissions.contains(&ModPermission::Recipes));
    assert_eq!(manifest.keys[1].modifiers, vec![Modifier::Ctrl]);
    assert_eq!(manifest.hash("ui/overlay.json"), Some(WASM));
    assert_eq!(manifest.hash(COMPONENT), Some(WASM));
}

#[test]
fn a_template_or_texture_without_a_hash_is_refused() {
    let error = refusal(&manifest(&format!(
        "templates = [\"ui/overlay.json\"]\n{}",
        files(&[])
    )));
    assert!(error.contains("ui/overlay.json"), "{error}");
    let error = refusal(&manifest(&format!(
        "textures = [\"textures/a.png\"]\n{}",
        files(&[])
    )));
    assert!(error.contains("textures/a.png"), "{error}");
}

#[test]
fn a_hashed_file_the_manifest_does_not_declare_is_refused() {
    let error = refusal(&manifest(&files(&["ui/stray.json"])));
    assert!(error.contains("ui/stray.json"), "{error}");
}

#[test]
fn the_component_needs_a_hash() {
    let error = refusal(&manifest("[files]\n"));
    assert!(error.contains(COMPONENT), "{error}");
}

#[test]
fn a_bad_hash_is_refused() {
    let error = refusal(&manifest("[files]\n\"mod.wasm\" = \"00\"\n"));
    assert!(error.contains("mod.wasm"), "{error}");
}

#[test]
fn an_action_or_key_outside_the_namespace_is_refused() {
    let error = refusal(&manifest(&format!(
        "actions = [\"other.next\"]\n{}",
        files(&[])
    )));
    assert!(error.contains("other.next"), "{error}");
    let error = refusal(&manifest(&format!(
        "[[keys]]\nid = \"other.show\"\nkey = \"r\"\nlabel = \"k\"\n{}",
        files(&[])
    )));
    assert!(error.contains("other.show"), "{error}");
}

#[test]
fn a_duplicate_key_is_refused() {
    let key = "[[keys]]\nid = \"bei.show\"\nkey = \"r\"\nlabel = \"k\"\n";
    let error = refusal(&manifest(&format!("{key}{key}{}", files(&[]))));
    assert!(error.contains("bei.show"), "{error}");
    let error = refusal(&manifest(&format!(
        "[[keys]]\nid = \"bei.a\"\nkey = \"r\"\nlabel = \"k\"\n\
         [[keys]]\nid = \"bei.b\"\nkey = \"r\"\nlabel = \"k\"\n{}",
        files(&[])
    )));
    assert!(error.contains("bei.b"), "{error}");
}

#[test]
fn unknown_keys_modifiers_permissions_and_api_are_refused() {
    let error = refusal(&manifest(&format!(
        "[[keys]]\nid = \"bei.a\"\nkey = \"space\"\nlabel = \"k\"\n{}",
        files(&[])
    )));
    assert!(error.contains("space"), "{error}");
    assert!(
        ModManifest::parse(&manifest(&format!(
            "[[keys]]\nid = \"bei.a\"\nkey = \"r\"\nmodifiers = [\"meta\"]\nlabel = \"k\"\n{}",
            files(&[])
        )))
        .is_err()
    );
    assert!(
        ModManifest::parse(&format!(
            "id = \"bei\"\nversion = \"0.1.0\"\napi = \"{API}\"\npermissions = [\"world\"]\n{}",
            files(&[])
        ))
        .is_err()
    );
    let error = refusal(&format!(
        "id = \"bei\"\nversion = \"0.1.0\"\napi = \"0.9\"\npermissions = []\n{}",
        files(&[])
    ));
    assert!(error.contains("0.9"), "{error}");
}

#[test]
fn paths_stay_inside_the_package() {
    for path in [
        "ui/../mod.json",
        "ui/Overlay.json",
        "textures/../../a.png",
        "textures/a.bmp",
    ] {
        let field = if path.starts_with("ui/") {
            "templates"
        } else {
            "textures"
        };
        let error = refusal(&manifest(&format!(
            "{field} = [\"{path}\"]\n{}",
            files(&[path])
        )));
        assert!(error.contains(path), "{path}: {error}");
    }
}

#[test]
fn an_id_that_is_not_a_lowercase_name_is_refused() {
    let error = refusal(&format!(
        "id = \"Bei Mod\"\nversion = \"0.1.0\"\napi = \"{API}\"\npermissions = []\n{}",
        files(&[])
    ));
    assert!(error.contains("Bei Mod"), "{error}");
}

/// JEI's recipe view keys: Backspace for history, Page Up and Page Down (and Shift with them) for
/// pages, and Control combinations.
#[test]
fn named_keys_and_modifier_combinations_parse() {
    let keys: String = [
        ("back", "backspace", ""),
        ("prev", "page_up", ""),
        ("next", "page_down", ""),
        ("prev_category", "page_up", "\"shift\""),
        ("find", "f", "\"ctrl\""),
    ]
    .iter()
    .map(|(id, key, modifiers)| {
        format!(
            "[[keys]]\nid = \"bei.{id}\"\nkey = \"{key}\"\nmodifiers = [{modifiers}]\nlabel = \"k\"\n"
        )
    })
    .collect();
    let parsed = ModManifest::parse(&manifest(&format!("{keys}{}", files(&[])))).unwrap();
    assert_eq!(parsed.keys.len(), 5);
    assert_eq!(parsed.keys[3].modifiers, [Modifier::Shift]);
    let error = refusal(&manifest(&format!(
        "[[keys]]\nid = \"bei.a\"\nkey = \"pageup\"\nlabel = \"k\"\n{}",
        files(&[])
    )));
    assert!(error.contains("pageup"), "{error}");
}

#[test]
fn declarations_parse_before_the_files_are_hashed() {
    let text = manifest(
        "actions = [\"bei.next\"]
[files]
",
    );
    assert!(ModManifest::parse(&text).is_err());
    assert_eq!(
        ModManifest::parse_declarations(&text).unwrap().actions,
        ["bei.next"]
    );
    assert!(
        ModManifest::parse_declarations(&manifest(
            "actions = [\"x.next\"]
"
        ))
        .is_err()
    );
}
