//! Wraps a WASI preview 1 reactor core module, such as a language runtime's shared-library build,
//! into a `packet-mod` component.

use anyhow::{Context, Result};
use wasi_preview1_component_adapter_provider::{
    WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME, WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
};
use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

/// The WIT packages the `packet-mod` world resolves against, dependencies first; the same
/// directories the host bindings read.
const WIT_SOURCES: [(&str, &str); 4] = [
    (
        "server-experience/capabilities.wit",
        include_str!("../../../experience-sdk/wit/client/deps/server-experience/capabilities.wit"),
    ),
    (
        "session/session.wit",
        include_str!("../../../experience-sdk/wit/session/session.wit"),
    ),
    (
        "world-draw/drawing.wit",
        include_str!("../../../mod-api/wit/deps/world-draw/drawing.wit"),
    ),
    (
        "mod-api/extension.wit",
        include_str!("../../../mod-api/wit/extension.wit"),
    ),
];
/// The world a packet mod's core module targets.
const WORLD: &str = "packet-mod";
/// Custom sections that carry a core module's component type.
const COMPONENT_TYPE_SECTION: &str = "component-type";

/// Embeds the `packet-mod` world when the module carries no component type of its own, then adds
/// the WASI preview 1 reactor adapter, which runs the module's `_initialize` on instantiation.
///
/// The adapter allocates its state through the module's `cabi_realloc` on its first WASI call,
/// which may come from `_initialize` itself. A runtime whose allocator only works once it is
/// initialized, such as Go's, cannot be adapted; the host loads its core module directly.
pub fn packet_mod_component(core: &[u8]) -> Result<Vec<u8>> {
    let mut module = core.to_vec();
    if !has_component_type(core)? {
        let mut resolve = Resolve::default();
        let mut package = None;
        for (path, source) in WIT_SOURCES {
            package = Some(resolve.push_source(path, source)?);
        }
        let package = package.context("packet-mod WIT sources are empty")?;
        let world = resolve.select_world(&[package], Some(WORLD))?;
        embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8)?;
    }
    ComponentEncoder::default()
        .module(&module)?
        .adapter(
            WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME,
            WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
        )?
        .validate(true)
        .encode()
        .context("encode packet-mod component")
}

/// Whether a core module already names its component type, as language bindings embed it.
fn has_component_type(core: &[u8]) -> Result<bool> {
    for payload in wasmparser::Parser::new(0).parse_all(core) {
        if let wasmparser::Payload::CustomSection(section) = payload?
            && section.name().starts_with(COMPONENT_TYPE_SECTION)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
