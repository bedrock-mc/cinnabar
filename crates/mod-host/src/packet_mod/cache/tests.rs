use super::*;
use anyhow::bail;
use std::sync::Arc;

/// Compiles the ordinary relay fixture for cache lifetime and admission checks.
fn program() -> (Vec<u8>, ModGrants, CompiledPacketMod) {
    let bytes = wat::parse_str(
        include_str!("../tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", ""),
    )
    .unwrap();
    let grants = ModGrants {
        packets: true,
        ..Default::default()
    };
    let program = CompiledPacketMod::compile(&bytes, &grants).unwrap();
    (bytes, grants, program)
}

#[test]
fn admitted_programs_reuse_their_engine_but_require_matching_source_and_grants() {
    let (bytes, grants, program) = program();
    let cache = Mutex::new(None);
    let first = load_cached(&cache, &bytes, &grants, || Ok((program, 1))).unwrap();
    let reused = load_cached(&cache, &bytes, &grants, || panic!("compiled a cache hit")).unwrap();
    assert!(Arc::ptr_eq(&first.ticker, &reused.ticker));
    let changed = ModGrants {
        wasi: true,
        ..grants.clone()
    };
    assert!(load_cached(&cache, &bytes, &changed, || bail!("different grants")).is_err());
    assert!(
        load_cached(&cache, b"different source", &grants, || bail!(
            "invalid ABI"
        ))
        .is_err()
    );
    let retained = load_cached(&cache, &bytes, &grants, || {
        panic!("replaced an admitted program")
    })
    .unwrap();
    assert!(Arc::ptr_eq(&first.ticker, &retained.ticker));
    assert!(first.instantiate().is_ok());
    assert!(reused.instantiate().is_ok());
}

#[test]
fn oversized_native_images_do_not_evict_the_retained_program() {
    let (bytes, grants, program) = program();
    let cache = Mutex::new(None);
    load_cached(&cache, &bytes, &grants, || Ok((program.clone(), 1))).unwrap();
    load_cached(&cache, b"larger source", &grants, || {
        Ok((program.clone(), MAX_PACKET_MOD_BYTES + 1))
    })
    .unwrap();
    assert!(load_cached(&cache, b"larger source", &grants, || bail!("not retained")).is_err());
    let retained = load_cached(&cache, &bytes, &grants, || {
        panic!("evicted a bounded program")
    })
    .unwrap();
    assert!(Arc::ptr_eq(&program.ticker, &retained.ticker));
}
