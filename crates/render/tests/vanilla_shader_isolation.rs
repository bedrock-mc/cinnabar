//! Vanilla shader bytes from 134348eb, including RGB lighting and weighted variants.
use sha2::{Digest, Sha256};

/// Removes only Enhanced preprocessor blocks, preserving every vanilla byte.
fn vanilla_source(source: &str) -> String {
    let mut active = vec![true];
    let mut result = String::new();
    for line in source.split_inclusive('\n') {
        let directive = line.trim();
        if directive.starts_with("#ifdef ENHANCED") {
            active.push(false);
        } else if directive == "#else" && active.len() > 1 {
            let last = active.last_mut().unwrap();
            *last = !*last;
        } else if directive == "#endif" && active.len() > 1 {
            active.pop();
        } else if active.iter().all(|value| *value) {
            result.push_str(line);
        }
    }
    assert_eq!(active.len(), 1);
    result
}

#[test]
fn disabled_enhanced_preserves_vanilla_shader_bytes() {
    for (source, digest) in [
        (
            include_str!("../src/chunk.wgsl"),
            "0931cbb32cd4c8ff94a1b839b0c22b837a5dbb992314f887f50c00d050ae9642",
        ),
        (
            include_str!("../src/model.wgsl"),
            "717662986ec8c5e089f9e4be585244ae0c99002ab772dda0b324646cf63a8dce",
        ),
        (
            include_str!("../src/liquid.wgsl"),
            "315e9b9d1f19b0e1889bebdbf340b986215e190c1c62c3f72948b48ca6545f49",
        ),
        (
            include_str!("../src/lighting.wgsl"),
            "ae7faeb7acea6a967a4e68d925c158b1db65053b2cf3003a1bbea5e61f4eae2c",
        ),
        (
            include_str!("../src/biome_tint.wgsl"),
            "cd04b8248192849c55dc46217710c5abfd5395ddef875a183140ed95ef4002a0",
        ),
        (
            include_str!("../src/atmosphere.wgsl"),
            "432068b10e34141461042d1daca907e9327159f5ed9cb8781f4e75e8af3aee7e",
        ),
        (
            include_str!("../src/material.wgsl"),
            "70d1a13ff2e1414be8a72139408487910276c6b15e10ac44dcbd028584abec69",
        ),
        (
            include_str!("../src/actor.wgsl"),
            "88d0fe5aab6f43108e03730a79f1d207b38479e19bbd27c695700e36b29f308c",
        ),
        (
            include_str!("../src/dropped_item.wgsl"),
            "a5bc82c300e55eeaab410ab1e535163a7590cec7c997fdf4d838002ac3a1517a",
        ),
        (
            include_str!("../src/hand_rig.wgsl"),
            "80d79cf47ef81fa2fbf0e4c6f49308cf59cb73deb962fd9e36564aa9410682e8",
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(vanilla_source(source))),
            digest
        );
    }
}

/// Freeze the real base descriptors as well as testing specialization mutations.
#[test]
fn vanilla_base_pipeline_construction_matches_baseline() {
    let source = include_str!("../src/chunk/pipeline/layouts.rs");
    let start = source
        .find("        let descriptor = RenderPipelineDescriptor")
        .unwrap();
    let end = source[start..]
        .find("\n#[derive(Clone, Copy, PartialEq")
        .unwrap()
        + start;
    let construction: String = source[start..end]
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(construction)),
        "29060b55955d474bb2e7d7361c76488275a706e196e477cfb851fd190541b838"
    );
}
