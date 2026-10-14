//! From successive selections and arriving meshes to what a renderer draws.
//!
//! The selection a walk publishes is the target. A target section is ready once its mesh
//! result arrived; one with no record or no quads is ready and draws nothing. The shown set
//! changes in units that never leave a hole or draw a region twice: a shown section the
//! target refined is replaced once every target section under it is ready, a coarsened
//! region once its new section is ready, and a target section overlapping nothing shown as
//! soon as it is ready. A unit still waiting after [`SWAP_TIMEOUT_STEPS`] updates is
//! applied with what is ready. A shown section above level 0 that the target refined
//! because it overlaps resident chunk columns goes at once, with the children that are
//! ready, the rest appearing as they arrive: only level-0 sections may be drawn over
//! resident columns, and renderers hide the others there anyway. Each update takes units nearest first up to a byte budget
//! of new meshes, so a renderer uploads it in a bounded number of frames.
//!
//! Wanted meshes are the target and the shown sections, whose meshes stay current while
//! they are drawn; [`ShownSet::take_interest`] reports what to want and release.

use std::sync::Arc;

use crate::{
    draw::{NeighbourLevel, NeighbourLevels, order::distance_blocks},
    geometry::{Face, SectionKey, SectionPos},
    mesh::output::SectionMesh,
    select::{KeyMap, KeySet, resident::ResidentColumns},
};

/// Updates a unit waits for its meshes before it is applied with what is ready.
pub const SWAP_TIMEOUT_STEPS: u32 = 64;

/// A section to draw from now on with its mesh: new, or a new mesh of a drawn section.
#[derive(Debug, Clone)]
pub struct DrawnSection {
    pub pos: SectionPos,
    pub mesh: Arc<SectionMesh>,
    pub neighbours: NeighbourLevels,
}

/// A drawn section and the crack lists its faces draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShownSection {
    pub key: SectionKey,
    pub neighbours: NeighbourLevels,
}

/// One step of what a renderer draws. Apply it whole: remove, then upsert, then relink.
#[derive(Debug, Clone)]
pub struct DrawUpdate {
    /// Sections drawn from now on, new or with a new mesh, nearest first.
    pub upserts: Vec<DrawnSection>,
    /// Sections no longer drawn, in ascending key order.
    pub removed: Vec<SectionKey>,
    /// Drawn sections whose neighbour levels changed while their mesh did not.
    pub relinked: Vec<ShownSection>,
    /// Every section drawn after this update, in ascending key order.
    pub shown: Arc<[ShownSection]>,
}

impl DrawUpdate {
    /// Bytes of the new meshes.
    #[must_use]
    pub fn upload_bytes(&self) -> usize {
        self.upserts
            .iter()
            .map(|section| section.mesh.heap_bytes())
            .sum()
    }
}

/// Counters of a [`ShownSet`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShownStats {
    /// Target sections.
    pub target: usize,
    /// Target sections whose mesh has not arrived.
    pub waiting: usize,
    /// Shown sections, with or without quads.
    pub shown: usize,
    /// Shown sections with quads.
    pub drawn: usize,
    /// Units applied after [`SWAP_TIMEOUT_STEPS`] with some meshes missing.
    pub timed_out: u64,
    /// Refinements applied with some meshes missing because the refined section overlaps
    /// resident chunk columns.
    pub resident_swaps: u64,
}

/// One target section's mesh.
#[derive(Debug, Clone)]
enum Target {
    /// Not arrived.
    Waiting,
    /// Arrived with quads.
    Mesh(Arc<SectionMesh>),
    /// Arrived without a record or quads.
    Nothing,
    /// Shown; its mesh went to the renderer.
    Shown,
}

impl Target {
    fn is_ready(&self) -> bool {
        !matches!(self, Self::Waiting)
    }

    /// The arrived result of `mesh`.
    fn of(mesh: Option<Arc<SectionMesh>>) -> Self {
        mesh.filter(|mesh| !mesh.is_empty())
            .map_or(Self::Nothing, Self::Mesh)
    }
}

/// One shown section.
#[derive(Debug, Clone, Copy)]
struct Shown {
    has_mesh: bool,
    neighbours: NeighbourLevels,
    /// Updates it has waited, replaced by the target, for its replacement.
    waited: u32,
}

/// A change applied whole: see the module documentation.
#[derive(Debug, Default)]
struct Unit {
    removes: Vec<SectionKey>,
    adds: Vec<SectionKey>,
    timed_out: bool,
    /// Applied early, as the section it removes overlaps resident chunk columns.
    resident: bool,
}

/// See the module documentation.
#[derive(Debug, Default)]
pub struct ShownSet {
    target: KeyMap<Target>,
    shown: KeyMap<Shown>,
    /// New results for shown sections, not yet in an update; `None` when it lost its quads.
    replaced: KeyMap<Option<Arc<SectionMesh>>>,
    wanted: KeySet,
    want: Vec<SectionPos>,
    release: Vec<SectionKey>,
    /// Drawn sections a reset removed, not yet in an update.
    cleared: Vec<SectionKey>,
    /// Shown sections above level 0 overlapping the target's resident chunk columns.
    over_resident: KeySet,
    timed_out: u64,
    resident_swaps: u64,
}

impl ShownSet {
    /// Replaces the target with `selected`, chosen for the chunk columns in `resident`.
    pub fn set_target(&mut self, selected: &[SectionPos], resident: &ResidentColumns) {
        let mut target = KeyMap::default();
        for &pos in selected {
            let key = pos.key();
            let state = self
                .target
                .remove(&key)
                .unwrap_or(if self.shown.contains_key(&key) {
                    Target::Shown
                } else {
                    Target::Waiting
                });
            target.insert(key, state);
        }
        self.target = target;
        self.over_resident = self
            .shown
            .keys()
            .filter(|key| key.pos().level() != 0 && resident.overlaps(key.pos()))
            .copied()
            .collect();
        self.sync_wanted();
    }

    /// Forgets the target, what is shown and what is wanted without releasing anything, as
    /// when meshes of another dimension are wanted and the old ones are dropped wholesale;
    /// the next update removes every drawn section, and nothing shown before is kept.
    pub fn reset(&mut self) {
        self.cleared.extend(
            self.shown
                .drain()
                .filter(|(_, shown)| shown.has_mesh)
                .map(|(key, _)| key),
        );
        self.target.clear();
        self.replaced.clear();
        self.over_resident.clear();
        self.wanted.clear();
        self.want.clear();
        self.release.clear();
    }

    /// Records a mesh result: `None` or an empty mesh when the section draws nothing.
    /// Results of sections neither in the target nor shown are dropped.
    pub fn mesh_arrived(&mut self, key: SectionKey, mesh: Option<Arc<SectionMesh>>) {
        if self.shown.contains_key(&key) {
            self.replaced
                .insert(key, mesh.filter(|mesh| !mesh.is_empty()));
            return;
        }
        if let Some(state) = self.target.get_mut(&key) {
            *state = Target::of(mesh);
        }
    }

    /// Moves the sections to want meshed and to release into the given lists, in that
    /// order of application: wants first.
    pub fn take_interest(&mut self, want: &mut Vec<SectionPos>, release: &mut Vec<SectionKey>) {
        want.append(&mut self.want);
        release.append(&mut self.release);
    }

    /// Counters and sizes.
    #[must_use]
    pub fn stats(&self) -> ShownStats {
        ShownStats {
            target: self.target.len(),
            waiting: self
                .target
                .values()
                .filter(|state| !state.is_ready())
                .count(),
            shown: self.shown.len(),
            drawn: self.shown.values().filter(|shown| shown.has_mesh).count(),
            timed_out: self.timed_out,
            resident_swaps: self.resident_swaps,
        }
    }

    /// Applies the units that are ready, nearest to `camera` first, until their new meshes
    /// exceed `byte_budget` (at least one unit is applied); `None` when nothing changed.
    pub fn update(&mut self, camera: [f64; 3], byte_budget: usize) -> Option<DrawUpdate> {
        let mut units = self.ready_units();
        let mut order: Vec<(u64, SectionKey)> = units
            .keys()
            .map(|&root| (distance_blocks(camera, root.pos()), root))
            .collect();
        order.sort_unstable();
        let mut removed = std::mem::take(&mut self.cleared);
        let mut upserts = Vec::new();
        let mut bytes = 0;
        let mut changed = false;
        for (_, root) in order {
            let unit = units.remove(&root).expect("ordered from the units");
            let unit_bytes: usize = unit
                .adds
                .iter()
                .map(|key| match self.target.get(key) {
                    Some(Target::Mesh(mesh)) => mesh.heap_bytes(),
                    _ => 0,
                })
                .sum();
            if bytes != 0 && unit_bytes != 0 && bytes + unit_bytes > byte_budget {
                continue;
            }
            bytes += unit_bytes;
            self.timed_out += u64::from(unit.timed_out && !unit.resident);
            self.resident_swaps += u64::from(unit.timed_out && unit.resident);
            changed |= self.apply(&unit, &mut removed, &mut upserts);
        }
        changed |= self.apply_replacements(
            byte_budget.saturating_sub(bytes),
            &mut removed,
            &mut upserts,
        );
        for (key, shown) in &mut self.shown {
            shown.waited = if self.target.contains_key(key) {
                0
            } else {
                shown.waited.saturating_add(1)
            };
        }
        let relinked = if changed {
            self.relink(&mut upserts)
        } else {
            Vec::new()
        };
        self.sync_wanted();
        if removed.is_empty() && upserts.is_empty() && relinked.is_empty() {
            return None;
        }
        upserts.sort_by_key(|section: &DrawnSection| distance_blocks(camera, section.pos));
        removed.sort_unstable();
        let mut shown: Vec<ShownSection> = self
            .shown
            .iter()
            .filter(|(_, shown)| shown.has_mesh)
            .map(|(&key, shown)| ShownSection {
                key,
                neighbours: shown.neighbours,
            })
            .collect();
        shown.sort_unstable_by_key(|section| section.key);
        Some(DrawUpdate {
            upserts,
            removed,
            relinked,
            shown: shown.into(),
        })
    }

    /// Every unit that can be applied now, by its root: the shown section a refinement
    /// replaces, the target section a coarsening shows, or the section added or removed
    /// alone.
    fn ready_units(&self) -> KeyMap<Unit> {
        // Target sections under each ancestor: how many, and how many are ready.
        let mut under: KeyMap<(u32, u32)> = KeyMap::default();
        for (key, state) in &self.target {
            for ancestor in ancestors(key.pos()) {
                let counts = under.entry(ancestor.key()).or_default();
                counts.0 += 1;
                counts.1 += u32::from(state.is_ready());
            }
        }
        let mut shown_above = KeySet::default();
        for key in self.shown.keys() {
            shown_above.extend(ancestors(key.pos()).map(SectionPos::key));
        }
        let mut units: KeyMap<Unit> = KeyMap::default();
        for (&key, shown) in &self.shown {
            if self.target.contains_key(&key) {
                continue;
            }
            let timed_out = shown.waited >= SWAP_TIMEOUT_STEPS;
            if let Some(coarse) = ancestors(key.pos()).find(|a| self.target.contains_key(&a.key()))
            {
                let ready = self.target[&coarse.key()].is_ready();
                if ready || timed_out {
                    let unit = units.entry(coarse.key()).or_default();
                    unit.removes.push(key);
                    unit.timed_out |= !ready;
                    if ready && unit.adds.is_empty() {
                        unit.adds.push(coarse.key());
                    }
                }
            } else if let Some(&(total, ready)) = under.get(&key) {
                let resident = self.over_resident.contains(&key);
                if ready == total || timed_out || resident {
                    let unit = units.entry(key).or_default();
                    unit.removes.push(key);
                    unit.timed_out = ready != total;
                    unit.resident = resident;
                }
            } else {
                units.entry(key).or_default().removes.push(key);
            }
        }
        for (key, state) in &self.target {
            if !matches!(state, Target::Mesh(_) | Target::Nothing) || units.contains_key(key) {
                continue;
            }
            let pos = key.pos();
            if let Some(root) = ancestors(pos).find(|a| units.contains_key(&a.key())) {
                units
                    .get_mut(&root.key())
                    .expect("found above")
                    .adds
                    .push(*key);
            } else if !ancestors(pos).any(|a| self.shown.contains_key(&a.key()))
                && !shown_above.contains(key)
            {
                units.entry(*key).or_default().adds.push(*key);
            }
        }
        units
    }

    /// Applies `unit`, recording removed drawn sections and new meshes; true when the
    /// shown set changed.
    fn apply(
        &mut self,
        unit: &Unit,
        removed: &mut Vec<SectionKey>,
        upserts: &mut Vec<DrawnSection>,
    ) -> bool {
        let mut changed = false;
        for key in &unit.removes {
            self.replaced.remove(key);
            if let Some(shown) = self.shown.remove(key) {
                self.over_resident.remove(key);
                changed = true;
                if shown.has_mesh {
                    removed.push(*key);
                }
            }
        }
        for &key in &unit.adds {
            let Some(state) = self.target.get_mut(&key) else {
                continue;
            };
            let has_mesh = match std::mem::replace(state, Target::Shown) {
                Target::Mesh(mesh) => {
                    upserts.push(DrawnSection {
                        pos: key.pos(),
                        mesh,
                        neighbours: NeighbourLevels::NONE,
                    });
                    true
                }
                Target::Nothing => false,
                other => {
                    *state = other;
                    continue;
                }
            };
            self.shown.insert(
                key,
                Shown {
                    has_mesh,
                    neighbours: NeighbourLevels::NONE,
                    waited: 0,
                },
            );
            changed = true;
        }
        changed
    }

    /// Hands out new meshes of shown sections within `byte_budget` (at least one), and
    /// stops drawing those that lost their quads; true when a section started or stopped
    /// drawing.
    fn apply_replacements(
        &mut self,
        byte_budget: usize,
        removed: &mut Vec<SectionKey>,
        upserts: &mut Vec<DrawnSection>,
    ) -> bool {
        let mut bytes = 0;
        let mut changed = false;
        let keys: Vec<SectionKey> = self.replaced.keys().copied().collect();
        for key in keys {
            let Some(shown) = self.shown.get_mut(&key) else {
                self.replaced.remove(&key);
                continue;
            };
            match self.replaced.get(&key).expect("listed above") {
                Some(mesh) => {
                    let size = mesh.heap_bytes();
                    if bytes != 0 && bytes + size > byte_budget {
                        continue;
                    }
                    bytes += size;
                    changed |= !shown.has_mesh;
                    shown.has_mesh = true;
                    upserts.push(DrawnSection {
                        pos: key.pos(),
                        mesh: Arc::clone(mesh),
                        neighbours: shown.neighbours,
                    });
                }
                None => {
                    if shown.has_mesh {
                        removed.push(key);
                        changed = true;
                    }
                    shown.has_mesh = false;
                }
            }
            self.replaced.remove(&key);
        }
        changed
    }

    /// Recomputes every drawn section's neighbour levels: sets them on `upserts` and
    /// returns the other drawn sections whose levels changed.
    fn relink(&mut self, upserts: &mut [DrawnSection]) -> Vec<ShownSection> {
        let mut shown_above = KeySet::default();
        for key in self.shown.keys() {
            shown_above.extend(ancestors(key.pos()).map(SectionPos::key));
        }
        let levels: Vec<(SectionKey, NeighbourLevels)> = self
            .shown
            .iter()
            .filter(|(_, shown)| shown.has_mesh)
            .map(|(&key, _)| (key, self.neighbour_levels(key.pos(), &shown_above)))
            .collect();
        let upserted: KeySet = upserts.iter().map(|section| section.pos.key()).collect();
        let mut relinked = Vec::new();
        for (key, neighbours) in levels {
            let shown = self.shown.get_mut(&key).expect("listed above");
            let changed = shown.neighbours != neighbours;
            shown.neighbours = neighbours;
            if changed && !upserted.contains(&key) {
                relinked.push(ShownSection { key, neighbours });
            }
        }
        for section in upserts.iter_mut() {
            section.neighbours = self.shown[&section.pos.key()].neighbours;
        }
        relinked.sort_unstable_by_key(|section| section.key);
        relinked
    }

    /// How the region across each face of `pos` is drawn; `shown_above` holds every
    /// ancestor of a shown section.
    fn neighbour_levels(&self, pos: SectionPos, shown_above: &KeySet) -> NeighbourLevels {
        let mut levels = NeighbourLevels::NONE;
        for face in Face::ALL {
            let Some(across) = pos.neighbour(face) else {
                continue;
            };
            let level = if self.shown.contains_key(&across.key()) {
                NeighbourLevel::Same
            } else if ancestors(across).any(|a| self.shown.contains_key(&a.key())) {
                NeighbourLevel::Coarser
            } else if shown_above.contains(&across.key()) {
                NeighbourLevel::Finer
            } else {
                NeighbourLevel::None
            };
            levels = levels.with(face, level);
        }
        levels
    }

    /// Wants every target and shown section, releasing those no longer either.
    fn sync_wanted(&mut self) {
        let wanted: KeySet = self
            .target
            .keys()
            .chain(self.shown.keys())
            .copied()
            .collect();
        for key in &self.wanted {
            if !wanted.contains(key) {
                self.release.push(*key);
            }
        }
        for key in &wanted {
            if !self.wanted.contains(key) {
                self.want.push(key.pos());
            }
        }
        self.want.sort_unstable_by_key(|pos| pos.key());
        self.release.sort_unstable();
        self.wanted = wanted;
    }
}

/// The proper ancestors of `pos`, nearest first.
fn ancestors(pos: SectionPos) -> impl Iterator<Item = SectionPos> {
    std::iter::successors(pos.parent(), |pos| pos.parent())
}
