//! CPU cost of summarising, encoding and admitting one column.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use far_terrain::{ColumnPos, ColumnSummary, SummaryStore, summarise_column};

#[allow(dead_code)]
#[path = "../tests/it/fixture.rs"]
mod fixture;

use fixture::{AIR, FLOWER, GRASS, LEAVES, SAND, WATER, column, context, section};

/// Rolling ground with a lake, scattered trees and plants over solid sections.
fn terrain() -> far_terrain::ColumnSnapshot {
    let solid = section(|_, _, _| [SAND, AIR]);
    let surface = section(|x, y, z| {
        let ground = 4 + (x as u32 * 3 + z as u32 * 5) % 6;
        let y = u32::from(y);
        if x < 5 && z < 6 {
            return if y <= 3 {
                [SAND, AIR]
            } else if y <= 6 {
                [WATER, AIR]
            } else {
                [AIR, AIR]
            };
        }
        match y {
            _ if y < ground => [GRASS, AIR],
            _ if y == ground && (x + z) % 4 == 0 => [FLOWER, AIR],
            _ if y > ground + 3 && (x * z) % 7 == 1 => [LEAVES, AIR],
            _ => [AIR, AIR],
        }
    });
    let air = section(|_, _, _| [AIR, AIR]);
    column(
        (-4..4)
            .map(|y| (y, solid.clone()))
            .chain([(4, surface)])
            .chain((5..20).map(|y| (y, air.clone()))),
    )
}

fn summary_costs(c: &mut Criterion) {
    let context = context();
    let column = terrain();
    let summary = summarise_column(&context, &column);
    let encoded = summary.encode();
    c.bench_function("far_terrain/summarise_column", |b| {
        b.iter(|| summarise_column(black_box(&context), black_box(&column)));
    });
    c.bench_function("far_terrain/encode", |b| {
        b.iter(|| black_box(&summary).encode())
    });
    c.bench_function("far_terrain/decode", |b| {
        b.iter(|| ColumnSummary::decode(black_box(&encoded.bytes)));
    });
    c.bench_function("far_terrain/admit_unchanged", |b| {
        let mut store = SummaryStore::new(usize::MAX);
        store.admit(ColumnPos::new(0, 0), &summary);
        b.iter(|| store.admit(ColumnPos::new(0, 0), black_box(&summary)));
    });
}

criterion_group!(benches, summary_costs);
criterion_main!(benches);
