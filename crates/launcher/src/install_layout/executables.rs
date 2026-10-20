use super::Platform;

/// The existing legacy client name for each desktop platform.
pub(super) const fn core_filename(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "bedrock-core.exe",
        Platform::Linux | Platform::MacOs => "bedrock-core",
    }
}

/// The Dragonfly executable name from the same table used to stage release binaries.
pub(super) fn local_server_filename(platform: Platform) -> String {
    let name = include_str!("../../../../packaging/common/runtime-binaries.txt")
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            (fields.nth(1) == Some("LOCAL_SERVER")).then_some(name)
        })
        .expect("the runtime binary table contains the local server");
    match platform {
        Platform::Windows => format!("{name}.exe"),
        Platform::Linux | Platform::MacOs => name.to_owned(),
    }
}
