use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use cinnabar_cxb::{bundle, fixtures, keys, seed_cache};
use server_experience::crypto;

const USAGE: &str = "usage: cinnabar-cxb keygen <seed-file>
       cinnabar-cxb build --manifest <toml|json> --component <wasm> --publisher-seed <seed-file> --out <bundle.cxb>
       cinnabar-cxb seed-cache --cxb <bundle.cxb> --user-data <dir>
       cinnabar-cxb write-fixtures <dir>";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["keygen", path] => println!("public_key={}", keys::generate(Path::new(path))?),
        ["build", flags @ ..] => {
            let [manifest, component, seed, out] = options(
                flags,
                ["--manifest", "--component", "--publisher-seed", "--out"],
            )?;
            let source = bundle::Source::read(Path::new(manifest))?;
            let wasm = std::fs::read(component).with_context(|| format!("reading {component}"))?;
            let built = bundle::build(source, &wasm, &keys::read(Path::new(seed))?)?;
            std::fs::write(out, &built.bytes).with_context(|| format!("writing {out}"))?;
            println!(
                "sha256={}\nbytes={}",
                crypto::digest(&built.bytes),
                built.bytes.len()
            );
        }
        ["seed-cache", flags @ ..] => {
            let [cxb, user_data] = options(flags, ["--cxb", "--user-data"])?;
            let (digest, objects) = seed_cache(Path::new(cxb), Path::new(user_data))?;
            println!("sha256={digest}\nobjects={}", objects.display());
        }
        ["write-fixtures", dir] => fixtures::write(Path::new(dir))?,
        _ => bail!(USAGE),
    }
    Ok(())
}

/// Reads each named option exactly once, in any order.
fn options<'a, const N: usize>(args: &[&'a str], names: [&str; N]) -> Result<[&'a str; N]> {
    ensure!(args.len() == 2 * N, USAGE);
    let mut values = [None; N];
    for pair in args.chunks_exact(2) {
        let index = names
            .iter()
            .position(|name| *name == pair[0])
            .with_context(|| format!("unknown option {}\n{USAGE}", pair[0]))?;
        ensure!(
            values[index].replace(pair[1]).is_none(),
            "{} given twice",
            pair[0]
        );
    }
    // N distinct known names filled N slots.
    Ok(values.map(|value| value.unwrap_or_default()))
}
