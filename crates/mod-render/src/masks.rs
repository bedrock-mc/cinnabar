//! Owner-scoped, transactional union selections. No entity-ID texture crosses the sandbox.

use mod_api::drawing::{
    DrawingHandle, DrawingSession, MAX_MASK_ENTITIES, MAX_MASK_VERTICES, MAX_MASKS, MaskDepth,
    MaskEntity,
};
use std::{collections::BTreeMap, sync::Arc};

/// A typed admission failure, preserved across the component boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskError {
    Denied,
    StaleHandle,
    BudgetExceeded,
    Invalid,
}

/// One admitted selection. Coverage is the union of the body's admitted equipment layers.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mask {
    pub position: Option<[f32; 3]>,
    pub handle: DrawingHandle,
    pub session: DrawingSession,
    pub depth: MaskDepth,
    pub entities: Vec<MaskEntity>,
    pub style: Option<mod_api::drawing::MaskStyle>,
}

impl Mask {
    /// Rechecks immutable authority before extracting or drawing any actor coverage.
    pub fn authorized(&self) -> bool {
        self.session.connection != 0
            && self.session.permissions.entity_highlight
            && (self.depth == MaskDepth::Test || self.session.permissions.render_through_walls)
    }
}

/// The owner incarnation is process-unique. Slots are never reused, even after rollback.
pub struct MaskStore {
    owner: u64,
    next_slot: u64,
    session: DrawingSession,
    targets: BTreeMap<DrawingHandle, (MaskEntity, u64)>,
    candidates: BTreeMap<MaskEntity, u64>,
    committed: Arc<[Mask]>,
    staged: Option<Vec<Mask>>,
}

impl Default for MaskStore {
    /// Every mask producer shares the same namespace as retained resources.
    fn default() -> Self {
        Self::new(crate::identity::next_owner())
    }
}

impl MaskStore {
    /// Starts with no session authority and no allocated attachments.
    fn new(owner: u64) -> Self {
        Self {
            owner,
            next_slot: 0,
            session: DrawingSession::default(),
            targets: BTreeMap::new(),
            candidates: BTreeMap::new(),
            committed: Arc::from([]),
            staged: None,
        }
    }

    /// Clears all resources before installing a changed session or effective consent.
    pub fn bind(&mut self, session: DrawingSession) {
        self.revoke();
        self.session = session;
    }

    /// Replaces trusted candidates. Membership retains incarnation keys across temporary culling.
    pub fn publish_targets(&mut self, targets: &[(MaskEntity, u64)]) {
        self.targets
            .retain(|_, (key, _)| targets.iter().any(|(current, _)| current == key));
        self.candidates.clear();
        for &(key, vertices) in targets {
            if key.connection != self.session.connection || key.dimension != self.session.dimension
            {
                continue;
            }
            self.candidates.insert(key, vertices);
            if let Some((_, count)) = self.targets.values_mut().find(|(target, _)| *target == key) {
                *count = vertices;
            }
        }
    }

    /// Resolves only explicitly consented unfiltered observations; a frustum is not occlusion.
    pub fn entity(&mut self, runtime_id: u64) -> Result<DrawingHandle, MaskError> {
        let permissions = self.session.permissions;
        if !permissions.entity_highlight
            || !permissions.entity_observe
            || !permissions.render_through_walls
        {
            return Err(MaskError::Denied);
        }
        if let Some(handle) = self
            .targets
            .iter()
            .find_map(|(token, (key, _))| (key.runtime_id == runtime_id).then_some(*token))
        {
            return Ok(handle);
        }
        let (&key, &vertices) = self
            .candidates
            .iter()
            .find(|(key, _)| key.runtime_id == runtime_id)
            .ok_or(MaskError::StaleHandle)?;
        let handle = self.allocate();
        self.targets.insert(handle, (key, vertices));
        Ok(handle)
    }

    /// Creates an empty selection with immutable depth authority within the owner's quota.
    pub fn create(&mut self, depth: MaskDepth) -> Result<DrawingHandle, MaskError> {
        self.admit(depth)?;
        if self.current().len() >= MAX_MASKS {
            return Err(MaskError::BudgetExceeded);
        }
        let handle = self.allocate();
        let session = self.session;
        self.staged().push(Mask {
            position: None,
            handle,
            session,
            depth,
            entities: Vec::new(),
            style: None,
        });
        Ok(handle)
    }

    /// Validates every token and the complete vertex charge before replacing any membership.
    pub fn select(
        &mut self,
        handle: DrawingHandle,
        entities: &[DrawingHandle],
    ) -> Result<(), MaskError> {
        let index = self.index(handle)?;
        self.admit(self.current()[index].depth)?;
        if entities.len() > MAX_MASK_ENTITIES {
            return Err(MaskError::BudgetExceeded);
        }
        let mut selected = BTreeMap::new();
        for token in entities {
            let &(key, vertices) = self.targets.get(token).ok_or(MaskError::StaleHandle)?;
            selected.insert(key, vertices);
        }
        if selected
            .values()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))
            .is_none_or(|vertices| vertices > MAX_MASK_VERTICES)
        {
            return Err(MaskError::BudgetExceeded);
        }
        self.staged()[index].entities = selected.into_keys().collect();
        Ok(())
    }

    /// Repositions a single admitted actor without changing membership or lifecycle fences.
    pub fn set_position(
        &mut self,
        handle: DrawingHandle,
        position: Option<[f32; 3]>,
    ) -> Result<(), MaskError> {
        let index = self.index(handle)?;
        self.admit(self.current()[index].depth)?;
        if position.is_some_and(|p| {
            self.current()[index].entities.len() > 1
                || p.iter()
                    .any(|v| !v.is_finite() || v.abs() > mod_api::MAX_PRIMITIVE_COORDINATE)
        }) {
            return Err(MaskError::Invalid);
        }
        self.staged()[index].position = position;
        Ok(())
    }

    /// Releases only this owner's mask; entity tokens and foreign masks cannot alias it.
    pub fn remove(&mut self, handle: DrawingHandle) -> Result<(), MaskError> {
        let index = self.index(handle)?;
        self.staged().remove(index);
        Ok(())
    }

    /// Updates only bounded host appearance; it grants no arbitrary shader sampling.
    pub fn set_style(
        &mut self,
        handle: DrawingHandle,
        style: Option<mod_api::drawing::MaskStyle>,
    ) -> Result<(), MaskError> {
        let index = self.index(handle)?;
        self.admit(self.current()[index].depth)?;
        if style.is_some_and(|style| !style.valid()) {
            return Err(MaskError::Invalid);
        }
        self.staged()[index].style = style;
        Ok(())
    }

    /// Validates the complete declared binding list against this owner's live masks.
    pub fn bindings(&self, handles: &[DrawingHandle]) -> Result<(), MaskError> {
        if handles.len() > MAX_MASKS {
            return Err(MaskError::BudgetExceeded);
        }
        for &handle in handles {
            self.admit(self.current()[self.index(handle)?].depth)?;
        }
        Ok(())
    }

    /// Begins a fresh callback transaction without recycling issued tokens.
    pub fn begin(&mut self) {
        self.staged = None;
    }

    /// Publishes a successful transaction, preserving identity for unchanged selections.
    pub fn commit(&mut self) -> Arc<[Mask]> {
        if let Some(staged) = self.staged.take()
            && staged.as_slice() != self.committed.as_ref()
        {
            self.committed = staged.into();
        }
        Arc::clone(&self.committed)
    }

    /// Immediately invalidates handles and all pending output on trap, reload or revocation.
    pub fn revoke(&mut self) {
        self.targets.clear();
        self.candidates.clear();
        self.staged = None;
        self.committed = Arc::from([]);
        self.session = DrawingSession::default();
    }

    /// Allocates a token in a namespace shared by masks and target references.
    fn allocate(&mut self) -> DrawingHandle {
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .expect("drawing handle space exhausted");
        DrawingHandle {
            owner: self.owner,
            generation: self.owner,
            slot: self.next_slot,
        }
    }

    /// Checks effective session policy independently of any caller-provided token.
    fn admit(&self, depth: MaskDepth) -> Result<(), MaskError> {
        if self.session.connection == 0
            || !self.session.permissions.entity_highlight
            || (depth == MaskDepth::Always && !self.session.permissions.render_through_walls)
        {
            return Err(MaskError::Denied);
        }
        Ok(())
    }

    /// Locates a current mask by its full owner-incarnation token.
    fn index(&self, handle: DrawingHandle) -> Result<usize, MaskError> {
        self.current()
            .iter()
            .position(|mask| mask.handle == handle)
            .ok_or(MaskError::StaleHandle)
    }

    /// Reads staged state when a callback has already changed it.
    fn current(&self) -> &[Mask] {
        self.staged.as_deref().unwrap_or(&self.committed)
    }

    /// Copies retained state at most once in a mutating callback.
    fn staged(&mut self) -> &mut Vec<Mask> {
        self.staged.get_or_insert_with(|| self.committed.to_vec())
    }
}

/// Accounts for coverage, its resolve target, and private multisampled companion depth.
pub fn attachment_bytes(width: u32, height: u32, samples: u32) -> Option<u64> {
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?
        .checked_mul(u64::from(samples).checked_mul(2)? + u64::from(samples > 1))
}

/// Lists the physical dimensions of every downsampled halo attachment, including pixel tails.
pub fn halo_sizes(
    mut width: u32,
    mut height: u32,
) -> [[u32; 2]; mod_api::drawing::MASK_BLUR_LEVELS] {
    std::array::from_fn(|_| {
        width = width.div_ceil(2).max(1);
        height = height.div_ceil(2).max(1);
        [width, height]
    })
}

/// Charges the exact shared halo pyramid using the same dimensions as its allocator.
pub fn halo_bytes(width: u32, height: u32) -> Option<u64> {
    halo_sizes(width, height)
        .into_iter()
        .try_fold(0u64, |sum, [width, height]| {
            sum.checked_add(
                u64::from(width)
                    .checked_mul(u64::from(height))?
                    .checked_mul(4)?,
            )
        })
}

#[cfg(test)]
mod tests;
