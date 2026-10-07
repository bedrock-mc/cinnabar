use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use cinnabar_cxb::{bundle, fixtures, keys, media, seed_cache};
use server_experience::crypto;

const USAGE: &str = "usage: cinnabar-cxb keygen <seed-file>
       cinnabar-cxb build --manifest <toml|json> --component <wasm> --publisher-seed <seed-file> --out <bundle.cxb> [--assets <dir>]
       cinnabar-cxb media --webm <in.webm> --url <https url> --id <media id> --poster <bundle path> --out-webm <file> --out-descriptor <file>
       cinnabar-cxb seed-cache --cxb <bundle.cxb> --user-data <dir>
       cinnabar-cxb write-fixtures <dir>";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["keygen", path] => println!("public_key={}", keys::generate(Path::new(path))?),
        ["build", flags @ ..] => {
            const NAMES: [&str; 4] = ["--manifest", "--component", "--publisher-seed", "--out"];
            let ([manifest, component, seed, out], assets) = if flags.len() == 2 * (NAMES.len() + 1)
            {
                let [manifest, component, seed, out, assets] = options(
                    flags,
                    [
                        "--manifest",
                        "--component",
                        "--publisher-seed",
                        "--out",
                        "--assets",
                    ],
                )?;
                (
                    [manifest, component, seed, out],
                    bundle::read_assets(Path::new(assets))?,
                )
            } else {
                (options(flags, NAMES)?, Vec::new())
            };
            let source = bundle::Source::read(Path::new(manifest))?;
            let wasm = std::fs::read(component).with_context(|| format!("reading {component}"))?;
            let built = bundle::build(source, &wasm, &assets, &keys::read(Path::new(seed))?)?;
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
        ["media", flags @ ..] => {
            let [webm, url, id, poster, out_webm, out_descriptor] = options(
                flags,
                [
                    "--webm",
                    "--url",
                    "--id",
                    "--poster",
                    "--out-webm",
                    "--out-descriptor",
                ],
            )?;
            let mut bytes = std::fs::read(webm).with_context(|| format!("reading {webm}"))?;
            let voided = media::strip_tags(&mut bytes)?;
            let descriptor = media::descriptor(&bytes, url, id, poster)?;
            std::fs::write(out_webm, &bytes).with_context(|| format!("writing {out_webm}"))?;
            std::fs::write(out_descriptor, serde_json::to_vec(&descriptor)?)
                .with_context(|| format!("writing {out_descriptor}"))?;
            println!(
                "voided={voided}\nsha256={}\nbytes={}\nduration_us={}",
                descriptor.sha256, descriptor.bytes, descriptor.duration_us
            );
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
