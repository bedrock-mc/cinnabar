use super::*;
impl Store {
    /// Validates one procedural fragment after checking owner authority and compile quota.
    pub fn create_material(
        &mut self,
        source: &str,
        permissions: DrawingPermissions,
    ) -> Result<Handle, DrawError> {
        if !(permissions.world_draw && permissions.render_material) {
            return Err(error(ErrorCode::Denied, "material authority"));
        }
        if self.material_compiles >= mod_api::MAX_MATERIAL_COMPILES_PER_CALLBACK {
            return Err(error(ErrorCode::BudgetExceeded, "material compiles"));
        }
        self.material_compiles += 1;
        let shader = crate::material::compose(source)
            .map_err(|_| error(ErrorCode::Invalid, "material shader"))?;
        let material = Arc::new(crate::material::Material {
            source: source.into(),
            shader: shader.into(),
        });
        let handle = self.token()?;
        self.edit(source.len(), |resources| {
            resources.materials.insert(handle, material);
            resources.material_revision += 1;
            Ok(())
        })?;
        Ok(handle)
    }
    /// Charges materials held by old render snapshots until their native pipelines retire.
    pub(super) fn material_budget(&self, candidate: &Resources) -> Result<(), DrawError> {
        let retiring = self
            .retired_materials
            .iter()
            .filter(|m| {
                Arc::strong_count(m) > 1
                    && !candidate
                        .materials
                        .values()
                        .any(|live| Arc::ptr_eq(m, live))
            })
            .count();
        if candidate.materials.len() + retiring > mod_api::MAX_DRAW_MATERIALS {
            return Err(error(ErrorCode::BudgetExceeded, "materials"));
        }
        Ok(())
    }
}
