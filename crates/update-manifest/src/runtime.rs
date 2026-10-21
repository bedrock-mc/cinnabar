//! Installed executable names from the release packaging table.

/// Resolves a packaging role to its executable filename on the requested platform.
pub fn executable(role: &str, os: &str) -> String {
    let name = include_str!("../../../packaging/common/runtime-binaries.txt")
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            fields.next()?;
            (fields.next()? == role).then_some(name)
        })
        .expect("packaged runtime role must exist");
    format!("{name}{}", if os == "windows" { ".exe" } else { "" })
}
