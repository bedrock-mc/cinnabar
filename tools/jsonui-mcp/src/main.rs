//! A stdio MCP server over the JSON-UI editor core: load packs, list screens,
//! resolve with provenance, validate, lay out and render to PNG, headlessly.
//! Speaks newline-delimited JSON-RPC 2.0.

#[cfg(test)]
mod tests;
mod tools;

fn main() {
    mcp_stdio::serve(&mut tools::Server::new(font_argument()));
}

/// `--font <carrier>`: the compiled Cinnangles Sans carrier text measures and draws with.
fn font_argument() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--font" {
            return args.next();
        }
        if let Some(path) = arg.strip_prefix("--font=") {
            return Some(path.to_owned());
        }
    }
    None
}
