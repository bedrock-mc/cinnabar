use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{ArchitectureError, paths::relative_slash, policy::Policy};

/// Checks executable headers and explicit Git binary attributes, regardless of the filename.
pub(super) fn check_binary_artifacts(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let binary_paths = binary_attributes(root, files)?;
    for path in files {
        let relative = relative_slash(root, path);
        let mut header = [0; 8];
        let length = File::open(path)
            .and_then(|mut file| file.read(&mut header))
            .map_err(|source| ArchitectureError::Read {
                path: path.clone(),
                source,
            })?;
        let executable = executable_header(&header[..length]);
        let owned = policy
            .owned_artifacts
            .iter()
            .any(|owned| relative.starts_with(&owned.path));
        if executable || (binary_paths.contains(&relative) && !owned) {
            let reason = if executable {
                "executable header"
            } else {
                "Git binary attribute"
            };
            diagnostics.push(format!("{relative}: forbidden binary artifact ({reason})"));
        }
    }
    Ok(())
}

/// Recognizes ELF, PE/DOS, thin and universal Mach-O, and WebAssembly executable formats.
fn executable_header(header: &[u8]) -> bool {
    header.starts_with(b"\x7fELF")
        || header.starts_with(b"MZ")
        || header.starts_with(b"\0asm")
        || [
            b"\xfe\xed\xfa\xce",
            b"\xce\xfa\xed\xfe",
            b"\xfe\xed\xfa\xcf",
            b"\xcf\xfa\xed\xfe",
            b"\xca\xfe\xba\xbe",
            b"\xbe\xba\xfe\xca",
            b"\xca\xfe\xba\xbf",
            b"\xbf\xba\xfe\xca",
        ]
        .iter()
        .any(|magic| header.starts_with(*magic))
}

/// Reads attributes in one Git invocation; filesystem-only fixtures have no Git attributes.
fn binary_attributes(
    root: &Path,
    files: &[PathBuf],
) -> Result<std::collections::BTreeSet<String>, ArchitectureError> {
    if !root.join(".git").exists() {
        return Ok(Default::default());
    }
    let result = (|| -> std::io::Result<_> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["check-attr", "-z", "--stdin", "binary"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let input = files
            .iter()
            .map(|path| relative_slash(root, path))
            .collect::<Vec<_>>()
            .join("\0")
            + "\0";
        let mut stdin = child.stdin.take().expect("piped Git stdin");
        // Drain output while writing input so large repositories cannot fill both pipes.
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output()?;
        writer
            .join()
            .map_err(|_| std::io::Error::other("Git attribute input thread failed"))??;
        if !output.status.success() {
            return Err(std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .collect::<Vec<_>>()
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|entry| entry[2] == b"set")
            .map(|entry| String::from_utf8_lossy(entry[0]).into_owned())
            .collect())
    })();
    result.map_err(|source| ArchitectureError::Read {
        path: root.join(".gitattributes"),
        source,
    })
}
