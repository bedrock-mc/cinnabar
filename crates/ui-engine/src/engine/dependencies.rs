//! Bounded logical dependencies of immutable layout output, before live source resolution.

use std::{
    collections::HashSet,
    mem::{align_of, size_of},
    sync::{Arc, Weak},
};

use json_ui::{Draw, DrawNode};

use super::{ScreenArt, banner_renderer, screen::CustomRenderer, tooltip};

// Keep a few screen/omission projections, not every layout visited by a long session.
const MAX_ENTRIES: usize = 8;
// Accounts for strings, summary/entry storage and Weak-retained node backing. The fixed
// eight-slot Vec and allocator bookkeeping are additional bounded overhead.
const MAX_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct DependencyCache {
    entries: Vec<Entry>,
    bytes: usize,
    byte_limit: usize,
    #[cfg(test)]
    discoveries: usize,
    #[cfg(test)]
    node_visits: usize,
    #[cfg(test)]
    banner_reads: usize,
}

struct Entry {
    nodes: Weak<[DrawNode]>,
    omissions: Box<[Box<str>]>,
    summary: Arc<Summary>,
    bytes: usize,
}

struct Summary {
    paths: Box<[Box<str>]>,
    renderers: Box<[Box<str>]>,
}

#[derive(Default)]
struct Collected<'a> {
    paths: HashSet<&'a str>,
    renderers: HashSet<&'a str>,
    #[cfg(test)]
    node_visits: usize,
    #[cfg(test)]
    banner_reads: usize,
}

pub(super) struct Dependencies<'a>(Storage<'a>);

enum Storage<'a> {
    Cached(Arc<Summary>),
    Uncached(Collected<'a>),
}

impl Default for DependencyCache {
    /// Starts an empty cache with fixed slot and retained-payload limits.
    fn default() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ENTRIES),
            bytes: 0,
            byte_limit: MAX_BYTES,
            #[cfg(test)]
            discoveries: 0,
            #[cfg(test)]
            node_visits: 0,
            #[cfg(test)]
            banner_reads: 0,
        }
    }
}

impl DependencyCache {
    /// Reuses this exact immutable snapshot and omission values, or discovers them once.
    /// Oversized inputs return borrowed dependencies without retaining their allocation.
    pub(super) fn get<'a>(
        &mut self,
        nodes: &'a Arc<[DrawNode]>,
        art: ScreenArt<'_>,
    ) -> Dependencies<'a> {
        self.entries.retain(|entry| {
            let live = entry.nodes.strong_count() != 0;
            if !live {
                self.bytes -= entry.bytes;
            }
            live
        });
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.omissions.len() == art.omit_controls.len()
                && entry
                    .omissions
                    .iter()
                    .zip(art.omit_controls)
                    .all(|(old, current)| old.as_ref() == *current)
                && entry
                    .nodes
                    .upgrade()
                    .is_some_and(|old| Arc::ptr_eq(&old, nodes))
        }) {
            let entry = self.entries.remove(index);
            let summary = Arc::clone(&entry.summary);
            self.entries.push(entry);
            return Dependencies(Storage::Cached(summary));
        }
        let collected = Collected::new(nodes, art);
        #[cfg(test)]
        {
            self.discoveries += 1;
            self.node_visits += collected.node_visits;
            self.banner_reads += collected.banner_reads;
        }
        let Some(bytes) = collected
            .retained_bytes(nodes.len(), art.omit_controls)
            .filter(|bytes| *bytes <= self.byte_limit)
        else {
            return Dependencies(Storage::Uncached(collected));
        };
        while self.entries.len() == MAX_ENTRIES || self.bytes > self.byte_limit - bytes {
            self.bytes -= self.entries.remove(0).bytes;
        }
        let summary = Arc::new(Summary {
            paths: owned(collected.paths),
            renderers: owned(collected.renderers),
        });
        self.entries.push(Entry {
            nodes: Arc::downgrade(nodes),
            omissions: art
                .omit_controls
                .iter()
                .map(|name| Box::from(*name))
                .collect(),
            summary: Arc::clone(&summary),
            bytes,
        });
        self.bytes += bytes;
        Dependencies(Storage::Cached(summary))
    }
}

impl<'a> Collected<'a> {
    /// Visits every non-omitted node, including interaction-gated alternatives.
    fn new(nodes: &'a [DrawNode], art: ScreenArt<'_>) -> Self {
        let mut result = Self::default();
        for node in nodes {
            #[cfg(test)]
            {
                result.node_visits += 1;
            }
            if art.omits(node) {
                continue;
            }
            match &node.draw {
                Draw::Sprite { texture, .. } => {
                    result.paths.insert(texture);
                }
                Draw::Custom { renderer, data } => {
                    result.renderers.insert(renderer);
                    if renderer == tooltip::RENDERER {
                        result.paths.insert(tooltip::BACKGROUND_TEXTURE);
                    } else if renderer == banner_renderer::RENDERER {
                        #[cfg(test)]
                        {
                            result.banner_reads += 1;
                        }
                        result.paths.extend(banner_renderer::texture_paths(data));
                    }
                }
                _ => {}
            }
        }
        result
    }

    /// Charges owned strings/arrays and the raw Arc slice backing retained by a Weak.
    /// Node-owned values drop with the last strong reference and are not retained here.
    fn retained_bytes(&self, nodes: usize, omissions: &[&str]) -> Option<usize> {
        let fixed = size_of::<Entry>()
            .checked_add(size_of::<Summary>())?
            .checked_add(4 * size_of::<usize>())? // Two Arc reference-count headers.
            .checked_add(align_of::<DrawNode>())?;
        let backing = nodes.checked_mul(size_of::<DrawNode>())?;
        self.paths
            .iter()
            .copied()
            .chain(self.renderers.iter().copied())
            .chain(omissions.iter().copied())
            .try_fold(fixed.checked_add(backing)?, |total, value| {
                total
                    .checked_add(size_of::<Box<str>>())?
                    .checked_add(value.len())
            })
    }
}

impl Dependencies<'_> {
    /// Appends logical base paths without copying their strings.
    fn append_paths<'a>(&'a self, paths: &mut HashSet<&'a str>) {
        match &self.0 {
            Storage::Cached(summary) => paths.extend(summary.paths.iter().map(AsRef::as_ref)),
            Storage::Uncached(collected) => paths.extend(collected.paths.iter().copied()),
        }
    }

    /// Checks exact custom-renderer membership without revisiting draw nodes.
    fn has_renderer(&self, name: &str) -> bool {
        match &self.0 {
            Storage::Cached(summary) => summary
                .renderers
                .binary_search_by(|current| current.as_ref().cmp(name))
                .is_ok(),
            Storage::Uncached(collected) => collected.renderers.contains(name),
        }
    }
}

/// Freezes a deduplicated dependency set in sorted order for borrowed membership queries.
fn owned(values: HashSet<&str>) -> Box<[Box<str>]> {
    let mut values: Vec<Box<str>> = values.into_iter().map(Box::from).collect();
    values.sort_unstable();
    values.into_boxed_slice()
}

/// Merges current overlays and every live renderer callback with the retained base paths.
/// The caller still resolves sources and requires the resulting atlas keys each prepare.
pub(super) fn paths<'a>(
    base: &'a Dependencies<'_>,
    art: ScreenArt<'a>,
    renderers: &'a [&mut dyn CustomRenderer],
) -> HashSet<&'a str> {
    let overlay = Collected::new(art.overlay, art);
    let mut paths = HashSet::new();
    base.append_paths(&mut paths);
    paths.extend(overlay.paths.iter().copied());
    let drawn = |name: &str| base.has_renderer(name) || overlay.renderers.contains(name);
    for renderer in renderers {
        renderer.textures(&drawn, &mut |path| {
            paths.insert(path);
        });
    }
    paths
}

#[cfg(test)]
mod tests;
