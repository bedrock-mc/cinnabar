//! Recipe book entry icons, kept while the listed stacks and every icon source stay the same.
//! A survival book lists every craftable output, so resolving each icon on every frame
//! dominated the open inventory.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use ui::IconRef;

/// The icons last resolved for the recipe book, with the inputs they were resolved from.
#[derive(Default)]
pub(crate) struct BookIconCache {
    inputs: Option<Inputs>,
    icons: Arc<[Option<IconRef>]>,
    /// Icon resolutions run, for cache tests.
    #[cfg(test)]
    pub(crate) resolutions: usize,
}

struct Inputs {
    stacks: Vec<protocol::NetworkItemStack>,
    sources: BookIconSources,
}

/// Everything besides the stacks that decides an entry's icon.
#[derive(Clone, Default)]
pub struct BookIconSources {
    /// The item registry and compiled facts canonical resolution reads.
    pub registry: Option<Arc<()>>,
    pub catalog: Option<Arc<assets::RuntimeIconCatalog>>,
    pub session_icons: u64,
    /// Animated dials (clocks, compasses) show the current frame.
    pub dials: inventory::DialFrames,
}

impl BookIconSources {
    fn same(&self, other: &Self) -> bool {
        same(&self.registry, &other.registry)
            && same(&self.catalog, &other.catalog)
            && self.session_icons == other.session_icons
            && self.dials == other.dials
    }
}

impl std::fmt::Debug for BookIconCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookIconCache")
            .field("icons", &self.icons.len())
            .finish_non_exhaustive()
    }
}

impl BookIconCache {
    /// The icons of `stacks` in order, resolving them through `icon` only when an input changed.
    pub(crate) fn icons(
        &mut self,
        stacks: Vec<protocol::NetworkItemStack>,
        sources: BookIconSources,
        icon: impl Fn(&protocol::NetworkItemStack) -> Option<IconRef>,
    ) -> Arc<[Option<IconRef>]> {
        let reusable = self
            .inputs
            .as_ref()
            .is_some_and(|inputs| inputs.sources.same(&sources) && inputs.stacks == stacks);
        if !reusable {
            self.icons = stacks.iter().map(&icon).collect();
            self.inputs = Some(Inputs { stacks, sources });
            #[cfg(test)]
            {
                self.resolutions += 1;
            }
        }
        Arc::clone(&self.icons)
    }
}

/// Compares retained allocations by identity, so a reused address never aliases a key.
fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}
