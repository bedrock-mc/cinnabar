use super::*;
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
use zip::{ZipWriter, write::SimpleFileOptions};

/// Builds synthetic UI layers without external pack assets.
fn stack(files: &[(&str, &[u8])]) -> Arc<ValidatedPackStack> {
    super::super::pack_reload_tests::stack(files)
}

/// Resolves the same per-pack UI order used by runtime catalog publication.
fn control_text(stack: &ValidatedPackStack) -> String {
    let mut catalog = json_ui::Catalog::default();
    for pack in stack.packs() {
        let files = pack
            .files_under("ui/")
            .iter()
            .map(|path| ((*path).to_owned(), pack.read_file(path).unwrap().unwrap()))
            .collect::<Vec<_>>();
        catalog.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_ref())),
        );
    }
    catalog.lookup("test", "control").unwrap().props["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn layer_boundaries_invalidate_ui_when_per_pack_ui_defs_change_effective_order() {
    let defs = br#"{"ui_defs":["ui/b.json","ui/a.json"]}"#;
    let a = br#"{"namespace":"test","control":{"type":"label","text":"a"}}"#;
    let b = br#"{"namespace":"test","control":{"type":"label","text":"b"}}"#;
    let together = stack(&[
        ("ui/_ui_defs.json", defs),
        ("ui/a.json", a),
        ("ui/b.json", b),
    ]);
    let lower = stack(&[("ui/_ui_defs.json", defs), ("ui/b.json", b)]);
    let higher = stack(&[("ui/a.json", a)]);
    let split = ValidatedPackStack::compose(&lower, &higher).unwrap();
    assert_eq!(control_text(&together), "b");
    assert_eq!(control_text(&split), "a");
    let previous = PackApplication {
        admission: PackAdmission::Validated(together),
        ..Default::default()
    };
    assert!(Changes::between(&split, Some(&previous)).ui);
}

#[test]
fn empty_layers_leave_subscriber_fingerprints_unchanged() {
    let source = stack(&[("ui/test.json", b"{}")]);
    let empty = stack(&[]);
    let together = ValidatedPackStack::compose(&source, &empty).unwrap();
    let previous = PackApplication {
        admission: PackAdmission::Validated(source),
        ..Default::default()
    };
    assert!(!Changes::between(&together, Some(&previous)).ui);
}

#[test]
fn selected_subpack_logical_content_changes_the_fingerprint() {
    let id = "00000000-0000-0000-0000-000000000033";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}],"subpacks":[{{"folder_name":"high","name":"High","memory_tier":2}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("ui/test.json", b"root".as_slice()),
        ("subpacks/high/ui/test.json", b"high".as_slice()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    let admit = |subpack: &str| {
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                subpack.into(),
                bytes.clone(),
            ),
        ]))
    };
    let root = admit("");
    let high = admit("high");
    let previous = PackApplication {
        admission: PackAdmission::Validated(root),
        ..Default::default()
    };
    assert!(Changes::between(&high, Some(&previous)).ui);
}

#[test]
fn consumed_texture_changes_only_its_subscriber() {
    let before = stack(&[
        ("textures/shared.png", b"old"),
        ("textures/unrelated.png", b"old"),
    ]);
    let unrelated = stack(&[
        ("textures/shared.png", b"old"),
        ("textures/unrelated.png", b"new"),
    ]);
    let consumed = stack(&[
        ("textures/shared.png", b"new"),
        ("textures/unrelated.png", b"old"),
    ]);
    let mut dependencies = Dependencies::default();
    compile(Subscriber::Entities, &before, &mut dependencies, |view| {
        assert_eq!(view.read("textures/shared.png").unwrap().as_ref(), b"old");
    });
    let previous = PackApplication {
        dependencies,
        admission: PackAdmission::Validated(before),
        ..Default::default()
    };
    assert!(!Changes::between(&unrelated, Some(&previous)).entities);
    assert!(Changes::between(&consumed, Some(&previous)).entities);
}

#[test]
fn absent_fallback_read_detects_a_new_override() {
    let before = stack(&[]);
    let after = stack(&[("textures/custom/skin.png", b"new")]);
    let mut dependencies = Dependencies::default();
    compile(Subscriber::Entities, &before, &mut dependencies, |view| {
        assert!(view.read("textures/custom/skin.png").is_none());
    });
    let previous = PackApplication {
        dependencies,
        admission: PackAdmission::Validated(before),
        ..Default::default()
    };
    assert!(Changes::between(&after, Some(&previous)).entities);
}

#[test]
fn directory_discovery_tracks_names_without_consuming_unread_pixels() {
    let before = stack(&[("textures/unread.png", b"old")]);
    let bytes_only = stack(&[("textures/unread.png", b"new")]);
    let added = stack(&[
        ("textures/unread.png", b"old"),
        ("textures/new.png", b"new"),
    ]);
    let mut dependencies = Dependencies::default();
    compile(Subscriber::Ui, &before, &mut dependencies, |view| {
        assert_eq!(view.list("textures/"), ["textures/unread.png"]);
    });
    let previous = PackApplication {
        dependencies,
        admission: PackAdmission::Validated(before),
        ..Default::default()
    };
    assert!(!Changes::between(&bytes_only, Some(&previous)).ui);
    assert!(Changes::between(&added, Some(&previous)).ui);
}

#[test]
fn missing_layer_read_on_empty_stack_is_still_a_dependency() {
    let before = stack(&[]);
    let after = stack(&[("textures/terrain_texture.json", b"{}")]);
    let mut dependencies = Dependencies::default();
    compile(Subscriber::Blocks, &before, &mut dependencies, |view| {
        assert!(view.read_layers("textures/terrain_texture.json").is_empty());
    });
    let previous = PackApplication {
        dependencies,
        admission: PackAdmission::Validated(before),
        ..Default::default()
    };
    assert!(Changes::between(&after, Some(&previous)).blocks);
}

// An unchanged part is reused without polling; a cancelled one never starts its compile.
#[test]
fn compile_part_reuses_unchanged_output_and_skips_cancelled_work() {
    let stack = stack(&[("texts/en_US.lang", b"a=b")]);
    let polls = std::sync::atomic::AtomicUsize::new(0);
    let poll = |answer| {
        let polls = &polls;
        move || {
            polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            answer
        }
    };
    let reused = compile_part(&stack, &poll(true), false, Subscriber::Ui, 1, |_| -> i32 {
        unreachable!("an unchanged subscriber is not compiled")
    });
    assert_eq!(reused, Some((1, None)));
    assert_eq!(polls.load(std::sync::atomic::Ordering::Relaxed), 0);
    let cancelled = compile_part(&stack, &poll(true), true, Subscriber::Ui, 1, |_| -> i32 {
        unreachable!("a cancelled subscriber is not compiled")
    });
    assert_eq!(cancelled, None);
    let (output, inputs) = compile_part(&stack, &poll(false), true, Subscriber::Ui, 1, |view| {
        view.read("texts/en_US.lang").map_or(0, |bytes| bytes.len())
    })
    .unwrap();
    assert_eq!(output, 3);
    assert!(inputs.unwrap().contains(&PackDependency::File {
        path: "texts/en_US.lang".into(),
        limit: resource_pack::MAX_FILE_BYTES,
    }));
}

#[test]
fn aim_highlight_overrides_track_only_their_texture_inputs() {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([7, 11, 13, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = format!("{}.png", render::AIM_ASSIST_TEXTURES[0]);
    let before = stack(&[(path.as_str(), png.get_ref())]);
    let mut dependencies = Dependencies::default();
    let textures = compile(
        Subscriber::AimAssist,
        &before,
        &mut dependencies,
        crate::camera::aim_highlight::prepare_pack_textures,
    );
    assert_eq!(
        textures[0].as_ref().unwrap().rgba.as_ref(),
        [7, 11, 13, 255]
    );
    assert!(textures[1].is_none());
    let previous = PackApplication {
        admission: PackAdmission::Validated(before),
        dependencies,
        aim_assist_textures: textures,
        ..Default::default()
    };
    let unrelated = stack(&[
        (path.as_str(), png.get_ref()),
        ("textures/unrelated.png", b"new"),
    ]);
    assert!(!Changes::between(&unrelated, Some(&previous)).aim_assist);
    let removed = stack(&[]);
    assert!(Changes::between(&removed, Some(&previous)).aim_assist);
}
