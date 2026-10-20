//! Device-sized output pages and the dispatches that touch each page.

use super::layout::ACTOR_VERTEX_STRIDE;
use super::skin::SkinJob;
use super::view::Dispatch;

/// One page's output interval and bounded skinning work.
pub(super) struct Page {
    pub first: u64,
    pub vertices: u32,
    pub jobs: Vec<SkinJob>,
    pub dispatches: Vec<Dispatch>,
}

/// Plans complete triangle pages without discarding any expanded vertices.
pub(super) fn plan(
    jobs: &[SkinJob],
    dispatches: &[Dispatch],
    vertices: u64,
    limit: u64,
) -> Vec<Page> {
    let capacity = (limit / ACTOR_VERTEX_STRIDE / 3) * 3;
    assert!(
        capacity >= 6,
        "actor output binding cannot hold a two-sided triangle"
    );
    let mut pages = Vec::new();
    let mut first = 0;
    while first < vertices {
        let end = (first + capacity).min(vertices);
        let mut page = Page {
            first,
            vertices: (end - first) as u32,
            jobs: Vec::new(),
            dispatches: Vec::new(),
        };
        for dispatch in dispatches {
            match *dispatch {
                Dispatch::Rig {
                    hand,
                    first: job_start,
                    count,
                    ..
                } => {
                    let start = page.jobs.len() as u32;
                    let mut max_vertices = 0;
                    for job in &jobs[job_start as usize..(job_start + count) as usize] {
                        let output = job.output_start();
                        if output < end && first < output + u64::from(job.output_vertices()) {
                            page.jobs.push(*job);
                            max_vertices = max_vertices.max(job.vertex_count);
                        }
                    }
                    let count = page.jobs.len() as u32 - start;
                    if count != 0 {
                        page.dispatches.push(Dispatch::Rig {
                            hand,
                            first: start,
                            count,
                            max_vertices,
                        });
                    }
                }
                Dispatch::BlockEntities {
                    first_output,
                    count,
                    ..
                } => {
                    if first_output < end && first < first_output + u64::from(count) {
                        page.dispatches.push(*dispatch);
                    }
                }
            }
        }
        pages.push(page);
        first = end;
    }
    pages
}

/// Grows a retained allocation without exceeding its device binding or buffer limit.
pub(super) fn allocation_size(required: u64, limit: u64) -> u64 {
    assert!(required <= limit);
    required
        .max(16)
        .next_multiple_of(16)
        .next_power_of_two()
        .min(limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_mesh_instances_page_without_exceeding_storage_bindings() {
        let mut output = 0u64;
        let jobs: Vec<_> = (0..128)
            .map(|instance| {
                let job = SkinJob {
                    instance,
                    first_output: output as u32,
                    first_output_high: (output >> 32) as u32,
                    vertex_count: 12_000,
                    ..Default::default()
                };
                output += u64::from(job.output_vertices());
                job
            })
            .collect();
        let limit = 128 * 1024 * 1024;
        let pages = plan(
            &jobs,
            &[Dispatch::Rig {
                hand: false,
                first: 0,
                count: 128,
                max_vertices: 12_000,
            }],
            output,
            limit,
        );
        assert!(pages.len() > 1);
        assert_eq!(
            pages.iter().map(|p| u64::from(p.vertices)).sum::<u64>(),
            output
        );
        for page in &pages {
            assert_eq!(page.first % 3, 0);
            assert!(
                allocation_size(u64::from(page.vertices) * ACTOR_VERTEX_STRIDE, limit) <= limit
            );
        }
        for job in jobs {
            let covered: u64 = pages
                .iter()
                .filter(|p| p.jobs.contains(&job))
                .map(|p| {
                    let start = p.first.max(job.output_start());
                    let end = (p.first + u64::from(p.vertices))
                        .min(job.output_start() + u64::from(job.output_vertices()));
                    end - start
                })
                .sum();
            assert_eq!(covered, u64::from(job.output_vertices()));
        }
    }

    #[test]
    fn output_pages_preserve_addresses_above_four_billion_vertices() {
        let start = u64::from(u32::MAX) - 2;
        let job = SkinJob {
            first_output: start as u32,
            first_output_high: (start >> 32) as u32,
            vertex_count: 6,
            ..Default::default()
        };
        assert_eq!(job.output_start(), start);
        assert_eq!(allocation_size(100, 112), 112);
    }
}
