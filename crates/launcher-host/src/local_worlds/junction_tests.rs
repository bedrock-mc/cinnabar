use super::*;

fn decode(buffer: &[u8]) -> (String, String) {
    let u16_at = |at: usize| u16::from_le_bytes([buffer[at], buffer[at + 1]]);
    let tag = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    assert_eq!(tag, REPARSE_TAG_MOUNT_POINT);
    assert_eq!(usize::from(u16_at(4)) + 8, buffer.len());
    let field = |offset: u16, length: u16| {
        let start = 16 + usize::from(offset);
        let units: Vec<u16> = (0..usize::from(length) / 2)
            .map(|index| u16_at(start + 2 * index))
            .collect();
        assert_eq!(u16_at(start + usize::from(length)), 0, "names end with NUL");
        String::from_utf16(&units).expect("UTF-16 name")
    };
    (field(u16_at(8), u16_at(10)), field(u16_at(12), u16_at(14)))
}

/// Folder names cmd.exe would mangle (`&`, `^`, `%`, spaces) reach the junction verbatim.
#[test]
fn junction_targets_carry_shell_metacharacters_verbatim() {
    for (target, substitute, print) in [
        (
            r"C:\Users\A & B\worlds\^x %PATH% é",
            r"\??\C:\Users\A & B\worlds\^x %PATH% é",
            r"C:\Users\A & B\worlds\^x %PATH% é",
        ),
        ("C:/Users/me/db", r"\??\C:\Users\me\db", r"C:\Users\me\db"),
        (r"\\?\D:\long path", r"\??\D:\long path", r"D:\long path"),
        (
            r"\\server\share\w&1",
            r"\??\UNC\server\share\w&1",
            r"\\server\share\w&1",
        ),
        (
            r"\\?\UNC\server\share",
            r"\??\UNC\server\share",
            r"\\server\share",
        ),
    ] {
        let buffer = junction_reparse_data(target).expect(target);
        assert_eq!(
            decode(&buffer),
            (substitute.to_owned(), print.to_owned()),
            "{target}"
        );
    }
    for relative in [r"relative\db", r"\rooted", "C:drive-relative", ""] {
        assert!(junction_reparse_data(relative).is_err(), "{relative:?}");
    }
}

/// The world link survives folder names cmd.exe mangles, and unlinking keeps the world data.
#[test]
fn world_links_reach_the_data_and_unlink_without_deleting_it() {
    let dir = crate::local_worlds::test_support::scratch();
    let root = dir.path().join("A & B ^ C % D %PATH%");
    let target = root.join("world & data");
    std::fs::create_dir_all(&target).expect("world folder");
    let link = root.join("install").join("worlds").join("id ^&%");
    crate::local_worlds::bds::link_world(&link, &target).expect("link");
    std::fs::write(link.join("level.dat"), b"x").expect("write through the link");
    assert_eq!(
        std::fs::read(target.join("level.dat")).expect("world data"),
        b"x"
    );
    crate::local_worlds::bds::unlink_world(&link);
    assert!(
        std::fs::symlink_metadata(link).is_err(),
        "the link survived"
    );
    assert!(
        target.join("level.dat").is_file(),
        "unlinking removed world data"
    );
}
