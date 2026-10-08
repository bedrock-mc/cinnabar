use std::io;
use std::path::Path;
use std::process;

use anyhow::{Context, bail};
use experience_runtime::protocol::fixtures;
use experience_runtime::serve::{EXIT_PROTOCOL, serve};

const USAGE: &str = "usage: experience-runtime serve [--report-fuel]
       experience-runtime write-fixtures <dir>";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let report_fuel = match args.as_slice() {
        ["serve"] => false,
        ["serve", "--report-fuel"] => true,
        ["write-fixtures", dir] => return write_fixtures(Path::new(dir)),
        _ => bail!(USAGE),
    };
    // The load-failed code promises a `load_failed` answer, so a frame that cannot be written
    // ends the session like one that cannot be read.
    let code = match serve(io::stdin().lock(), io::stdout().lock(), report_fuel) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("serve: writing a frame: {error:#}");
            EXIT_PROTOCOL
        }
    };
    process::exit(code)
}

fn write_fixtures(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (name, json) in fixtures() {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}
