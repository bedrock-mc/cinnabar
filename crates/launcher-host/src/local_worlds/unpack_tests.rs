use super::*;
use crate::local_worlds::test_support::{build_zip, scratch};

fn archive_file(dir: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("archive.zip");
    fs::write(&path, build_zip(files)).expect("archive");
    path
}

/// Parallel workers write every file into nested folders with exact contents.
#[test]
fn parallel_extraction_writes_every_file() {
    let dir = scratch();
    let files: Vec<(String, Vec<u8>)> = (0..400)
        .map(|index| {
            (
                format!("packs/p{}/sub{}/file{index}.json", index % 7, index % 13),
                index.to_string().repeat(index % 50 + 1).into_bytes(),
            )
        })
        .chain([("bedrock_server".to_owned(), b"elf".to_vec())])
        .collect();
    let listing: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_slice()))
        .collect();
    let archive = archive_file(dir.path(), &listing);
    let root = dir.path().join("root");
    extract_archive(&root, &archive, &BDS_LIMITS).expect("extract");
    for (name, body) in &files {
        assert_eq!(&fs::read(root.join(name)).expect(name), body, "{name}");
    }
}

/// Limits apply to the whole listing before anything is written.
#[test]
fn listings_over_the_limits_write_nothing() {
    let limits = UnpackLimits {
        max_entries: 3,
        max_file_bytes: 1000,
        max_total_bytes: 1500,
        min_ratio_sample: 8,
        max_entry_ratio: 20.0,
        max_archive_ratio: 10.0,
    };
    let mut random = vec![0u8; 1001];
    getrandom::fill(&mut random).expect("random bytes");
    let squeezable = vec![b'z'; 1000];
    let one: &[u8] = b"1";
    let cases: [(&str, Vec<(&str, &[u8])>); 6] = [
        (
            "entries",
            vec![("a", one), ("b", one), ("c", one), ("d", one)],
        ),
        ("file size", vec![("a", random.as_slice())]),
        ("total", vec![("a", &random[..900]), ("b", &random[100..])]),
        ("ratio", vec![("a", squeezable.as_slice())]),
        ("duplicate", vec![("Dir/File", one), ("dir/file", one)]),
        ("escape", vec![("../evil", one)]),
    ];
    for (case, files) in cases {
        let dir = scratch();
        let archive = archive_file(dir.path(), &files);
        let root = dir.path().join("root");
        fs::create_dir_all(&root).expect("root");
        assert!(
            extract_archive(&root, &archive, &limits).is_err(),
            "{case} accepted"
        );
        let written = fs::read_dir(root).expect("root").count();
        assert_eq!(written, 0, "{case} wrote before rejecting");
    }
}

#[test]
fn entry_names_stay_inside_the_install() {
    let root = Path::new("/install");
    assert_eq!(
        entry_target(root, "a/./b/../c").expect("inside"),
        root.join("a").join("c")
    );
    for name in ["/etc/passwd", "../x", "a/../../x"] {
        assert!(entry_target(root, name).is_err(), "{name}");
    }
    assert!(install_dir_name("1.26.52.3") && install_dir_name("1.26.41.0.partial"));
    assert!(!install_dir_name("not-a-version") && !install_dir_name("downloads"));
}
