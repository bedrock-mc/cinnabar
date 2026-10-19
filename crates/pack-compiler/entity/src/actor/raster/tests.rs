use super::*;
use crate::actor::{MAX_ACTOR_PIXEL_BYTES, MAX_ACTOR_TEXTURE_SIDE, decode_raster};
use image::{ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

/// Plans the complete input together, independently of incremental staging.
fn global_plan(rasters: &[(u32, u16, u16)], budget: usize, limit: usize) -> BTreeMap<u32, u8> {
    let mut queue = BinaryHeap::new();
    let mut total = 0usize;
    for &(source, width, height) in rasters.iter().take(limit) {
        let bytes = raster_bytes(width, height);
        total = total.saturating_add(bytes);
        queue.push((bytes, Reverse(source), width, height));
    }
    let mut halvings = BTreeMap::new();
    while total > budget {
        let Some((bytes, Reverse(source), width, height)) = queue.pop() else {
            break;
        };
        if width.max(height) <= 1 {
            break;
        }
        let (width, height) = (width.div_ceil(2), height.div_ceil(2));
        let halved = raster_bytes(width, height);
        total = total - bytes + halved;
        *halvings.entry(source).or_insert(0) += 1;
        queue.push((halved, Reverse(source), width, height));
    }
    halvings
}

/// Nonuniform pixels distinguish repeated box filtering from a direct resize.
fn patterned(width: u16, height: u16, seed: u32) -> DecodedRaster {
    DecodedRaster {
        width,
        height,
        pixels: (0..raster_bytes(width, height))
            .map(|index| ((index as u32 * 37 + seed * 23) % 256) as u8)
            .collect(),
    }
}

#[test]
fn streamed_sizes_pixels_and_hashes_equal_global_planning() {
    let shapes = [(1, 1), (1, 9), (9, 1), (3, 7), (7, 3), (8, 8), (15, 9)];
    for a in shapes {
        for b in shapes {
            for c in shapes {
                let rasters = [(9, a.0, a.1), (2, b.0, b.1), (7, c.0, c.1)];
                for budget in [12, 20, 64, 256] {
                    for limit in 1..=rasters.len() {
                        let expected_plan = global_plan(&rasters, budget, limit);
                        let calls = std::sync::Mutex::new(Vec::new());
                        let mut prepared = prepare_rasters(
                            rasters.iter().map(|&(source, _, _)| source),
                            |source| {
                                calls.lock().unwrap().push(source);
                                let &(_, width, height) = rasters
                                    .iter()
                                    .find(|&&(candidate, _, _)| candidate == source)
                                    .unwrap();
                                Ok(Some(patterned(width, height, source)))
                            },
                            budget,
                            limit,
                        );
                        assert_eq!(
                            {
                                let mut calls = calls.lock().unwrap().clone();
                                calls.sort_by_key(|source| {
                                    rasters
                                        .iter()
                                        .position(|raster| raster.0 == *source)
                                        .unwrap()
                                });
                                calls
                            },
                            rasters[..limit]
                                .iter()
                                .map(|raster| raster.0)
                                .collect::<Vec<_>>()
                        );
                        let mut textures = Vec::new();
                        let mut bytes = 0;
                        for &(source, width, height) in rasters[..limit].iter().rev() {
                            let mut expected = patterned(width, height, source);
                            for _ in 0..expected_plan.get(&source).copied().unwrap_or(0) {
                                expected.pixels =
                                    halve(&expected.pixels, expected.width, expected.height);
                                (expected.width, expected.height) =
                                    (expected.width.div_ceil(2), expected.height.div_ceil(2));
                            }
                            let raster = prepared.get_mut(&source).unwrap().as_mut().unwrap();
                            assert_eq!(
                                (raster.width, raster.height),
                                (expected.width, expected.height)
                            );
                            assert_eq!(&*raster.pixels, expected.pixels);
                            let index =
                                admit(&mut textures, &mut bytes, source, raster, budget).unwrap();
                            assert_eq!(
                                textures[index].pixel_sha256,
                                <[u8; 32]>::from(Sha256::digest(&expected.pixels))
                            );
                            assert_eq!(&*textures[index].rgba8, expected.pixels);
                            assert!(Arc::ptr_eq(&textures[index].rgba8, &raster.pixels));
                        }
                        assert!(bytes <= budget);
                    }
                }
            }
        }
    }
}

#[test]
fn rasters_past_the_budget_halve_the_largest_first() {
    let side = MAX_ACTOR_TEXTURE_SIDE;
    let rasters = [(1, side, side), (2, 4096, 4096), (3, 16, 16)];
    let expected = global_plan(&rasters, MAX_ACTOR_PIXEL_BYTES, MAX_ACTOR_TEXTURES);
    let mut plan = RasterPlan::default();
    let mut steps = BTreeMap::new();
    for (source, width, height) in rasters {
        plan.insert(source, width, height);
        while let Some(source) = plan.shrink(MAX_ACTOR_PIXEL_BYTES) {
            *steps.entry(source).or_insert(0) += 1;
        }
        assert!(plan.total <= MAX_ACTOR_PIXEL_BYTES);
    }
    assert_eq!(steps, expected);
    assert!(steps.get(&1).copied().unwrap_or(0) > 0);
    assert_eq!(steps.get(&3), None);
}

#[test]
fn a_raster_past_the_pixel_budget_is_halved_to_fit() {
    let mut raster = SharedRaster::from(DecodedRaster {
        width: 4,
        height: 2,
        pixels: vec![200; 4 * 2 * 4],
    });
    let mut textures = Vec::new();
    let mut pixel_bytes = MAX_ACTOR_PIXEL_BYTES - 8;
    assert_eq!(
        admit(
            &mut textures,
            &mut pixel_bytes,
            3,
            &mut raster,
            MAX_ACTOR_PIXEL_BYTES
        ),
        Some(0)
    );
    assert_eq!((textures[0].width, textures[0].height), (2, 1));
    assert_eq!(pixel_bytes, MAX_ACTOR_PIXEL_BYTES);
    assert!(Arc::ptr_eq(&textures[0].rgba8, &raster.pixels));
}

#[test]
fn unusable_images_do_not_affect_planning_or_the_usable_source_cutoff() {
    let mut encoded = Vec::new();
    for (width, height, alpha) in [(4, 4, 255), (8, 8, 127), (3, 7, 255)] {
        let mut bytes = Cursor::new(Vec::new());
        RgbaImage::from_pixel(width, height, Rgba([9, 25, 42, alpha]))
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        encoded.push(bytes.into_inner());
    }
    let broken_header = encoded[0][..33].to_vec();
    let images = [&encoded[0], &encoded[1], &broken_header, &encoded[2]];
    let calls = std::sync::Mutex::new(Vec::new());
    let prepared = prepare_rasters(
        0..4,
        |source| {
            calls.lock().unwrap().push(source);
            Ok(decode_raster(
                "fixture.png",
                images[source as usize],
                Some(&|width, height| Some(vec![true; usize::from(width) * usize::from(height)])),
                false,
            ))
        },
        64,
        2,
    );
    let mut calls = calls.into_inner().unwrap();
    calls.sort();
    assert_eq!(calls, [0, 1, 2, 3]);
    assert!(
        prepared[&1].is_none(),
        "sampled fractional alpha was accepted"
    );
    assert!(prepared[&2].is_none(), "a truncated image was accepted");
    let first = prepared[&0].as_ref().unwrap();
    let last = prepared[&3].as_ref().unwrap();
    assert_eq!((first.width, first.height), (2, 2));
    assert_eq!((last.width, last.height), (2, 4));
    assert_eq!(first.pixels.len() + last.pixels.len(), 48);
}

#[test]
fn a_failed_prepass_read_is_left_absent_for_the_rig_to_retry() {
    let prepared = prepare_rasters(
        [1, 2],
        |source| {
            if source == 1 {
                Err(crate::actor::invalid("read failed"))
            } else {
                Ok(Some(patterned(2, 2, source)))
            }
        },
        16,
        1,
    );
    assert!(!prepared.contains_key(&1));
    assert!(prepared[&2].is_some());
}

#[test]
fn parallel_decode_uses_the_callers_pool_and_preserves_pixels() {
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let barrier = std::sync::Barrier::new(threads);
                let prepared = prepare_rasters(
                    0..threads as u32,
                    |source| {
                        assert_eq!(rayon::current_num_threads(), threads);
                        barrier.wait();
                        Ok(Some(patterned(7, 9, source)))
                    },
                    128,
                    threads,
                );
                prepared
                    .into_iter()
                    .map(|(source, raster)| {
                        let raster = raster.unwrap();
                        (source, raster.width, raster.height, raster.pixels.to_vec())
                    })
                    .collect::<Vec<_>>()
            })
    };
    let parallel = run(4);
    let serial = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            prepare_rasters(0..4, |source| Ok(Some(patterned(7, 9, source))), 128, 4)
                .into_iter()
                .map(|(source, raster)| {
                    let raster = raster.unwrap();
                    (source, raster.width, raster.height, raster.pixels.to_vec())
                })
                .collect::<Vec<_>>()
        });
    assert_eq!(parallel, serial);
}
