//! Native core-export signatures and the grants needed to negotiate each callback.

use crate::ModGrants;
use anyhow::{Result, ensure};
use wasmparser::{
    CompositeInnerType, ExportSectionReader, ExternalKind, FunctionSectionReader,
    ImportSectionReader, Payload, TypeRef, TypeSectionReader, ValType,
};
use wasmtime::{ExternType, Module};

pub(super) const NATIVE_INIT: &str = "native-init";
pub(super) const RENDER_FRAME: &str = "render-frame";
pub(super) const CAMERA_FRAME: &str = "camera-frame";
pub(super) const FRAME: &str = "frame";
pub(super) const MENU: &str = "menu";

struct Export {
    name: &'static str,
    params: usize,
    results: usize,
    capability: u32,
    required: fn() -> ModGrants,
}

/// Every native export's ABI and authority live together; discovery never grants authority.
const EXPORTS: [Export; 5] = [
    Export {
        name: NATIVE_INIT,
        params: 1,
        results: 0,
        capability: 0,
        required: ModGrants::default,
    },
    Export {
        name: FRAME,
        params: 2,
        results: 1,
        capability: mod_api::native_frame::CAMERA_PLAYERS,
        required: || ModGrants {
            players: true,
            camera: true,
            ..Default::default()
        },
    },
    Export {
        name: CAMERA_FRAME,
        params: 2,
        results: 1,
        capability: mod_api::native_camera::CAPABILITY,
        required: || ModGrants {
            camera: true,
            movement: true,
            ..Default::default()
        },
    },
    Export {
        name: RENDER_FRAME,
        params: 2,
        results: 1,
        capability: mod_api::native_render::CAPABILITY,
        required: || ModGrants {
            world_draw: true,
            render_through_walls: true,
            ..Default::default()
        },
    },
    Export {
        name: MENU,
        params: 2,
        results: 1,
        capability: mod_api::native_menu::CONTROLS,
        required: || ModGrants {
            controls: true,
            ..Default::default()
        },
    },
];

#[derive(Clone, Copy)]
struct Signature {
    params: usize,
    results: usize,
    i32_only: bool,
}

/// Missing exports stay distinct from present exports with an invalid kind or signature.
#[derive(Default)]
pub(crate) struct NativeExports {
    signatures: [Option<bool>; EXPORTS.len()],
}

impl NativeExports {
    /// Reads the same export contract from the compiled module used for execution.
    pub(super) fn from_module(module: &Module) -> Self {
        let mut exports = Self::default();
        for export in &EXPORTS {
            if let Some(ty) = module.get_export(export.name) {
                let signature = match ty {
                    ExternType::Func(ty) => Some(Signature {
                        params: ty.params().len(),
                        results: ty.results().len(),
                        i32_only: ty.params().chain(ty.results()).all(|ty| ty.is_i32()),
                    }),
                    _ => None,
                };
                exports.record(export.name, signature);
            }
        }
        exports
    }

    /// Records only recognized top-level names, rejecting duplicate or mismatched exports.
    fn record(&mut self, name: &str, signature: Option<Signature>) {
        let Some(index) = EXPORTS.iter().position(|export| export.name == name) else {
            return;
        };
        let export = &EXPORTS[index];
        let valid = signature.is_some_and(|signature| {
            signature.params == export.params
                && signature.results == export.results
                && signature.i32_only
        });
        self.signatures[index] = Some(self.signatures[index].is_none() && valid);
    }

    /// Validates the complete optional ABI before exposing any native capability.
    fn validate(&self) -> Result<()> {
        let init = EXPORTS
            .iter()
            .zip(&self.signatures)
            .any(|(export, signature)| export.name == NATIVE_INIT && signature.is_some());
        let callback = EXPORTS
            .iter()
            .zip(&self.signatures)
            .any(|(export, signature)| export.capability != 0 && signature.is_some());
        ensure!(
            init == callback,
            "native-init requires frame or menu exports"
        );
        for (export, signature) in EXPORTS.iter().zip(&self.signatures) {
            ensure!(
                *signature != Some(false),
                "packet mod export {} has the wrong signature",
                export.name
            );
        }
        Ok(())
    }

    /// Computes callback bits only where the caller explicitly granted every required field.
    pub(super) fn negotiate(&self, granted: &ModGrants) -> Result<u32> {
        self.validate()?;
        let mut bits = EXPORTS
            .iter()
            .zip(&self.signatures)
            .filter(|(_, signature)| **signature == Some(true))
            .filter(|(export, _)| {
                let required = (export.required)();
                ModGrants::CAPABILITIES
                    .iter()
                    .all(|grant| (grant.requested_allowed)(granted, &required))
            })
            .fold(0, |bits, (export, _)| bits | export.capability);
        if bits & mod_api::native_render::CAPABILITY != 0 {
            if granted.entity_observe && granted.entity_highlight {
                bits |= mod_api::native_render::ENTITY_HIGHLIGHTS;
            }
            if granted.block_highlights {
                bits |= mod_api::native_render::BLOCK_HIGHLIGHTS;
            }
        }
        Ok(bits)
    }

    /// Synthesizes permission requests for a valid ABI without changing any stored grants.
    pub(crate) fn requested(&self) -> ModGrants {
        let mut requested = ModGrants::default();
        if self.validate().is_err() {
            return requested;
        }
        for (export, signature) in EXPORTS.iter().zip(&self.signatures) {
            if *signature != Some(true) {
                continue;
            }
            let required = (export.required)();
            for grant in ModGrants::CAPABILITIES {
                if (grant.enabled)(&required) {
                    (grant.set)(&mut requested, true, Some(&required));
                }
            }
        }
        if requested.world_draw {
            requested.entity_observe = true;
            requested.entity_highlight = true;
            requested.block_highlights = true;
        }
        requested
    }
}

/// Borrowed section readers keep discovery memory independent of the module's function count.
#[derive(Default)]
pub(crate) struct Metadata<'a> {
    types: Option<TypeSectionReader<'a>>,
    imports: Option<ImportSectionReader<'a>>,
    functions: Option<FunctionSectionReader<'a>>,
    exports: Option<ExportSectionReader<'a>>,
    pub(crate) needs_wasi: bool,
}

impl<'a> Metadata<'a> {
    /// Retains top-level section views without compiling, instantiating, or copying bodies.
    pub(crate) fn observe(&mut self, payload: &Payload<'a>) -> Result<()> {
        match payload {
            Payload::TypeSection(types) => self.types = Some(types.clone()),
            Payload::ImportSection(imports) => {
                for import in imports.clone().into_imports() {
                    self.needs_wasi |= import?.module == "wasi_snapshot_preview1";
                }
                self.imports = Some(imports.clone());
            }
            Payload::FunctionSection(functions) => self.functions = Some(functions.clone()),
            Payload::ExportSection(exports) => self.exports = Some(exports.clone()),
            _ => {}
        }
        Ok(())
    }

    /// Resolves only the recognized exports through the module's two function-index spaces.
    pub(crate) fn requested(&self) -> Result<ModGrants> {
        let mut native = NativeExports::default();
        if let Some(exports) = &self.exports {
            for export in exports.clone() {
                let export = export?;
                if !EXPORTS.iter().any(|known| known.name == export.name) {
                    continue;
                }
                let signature = if export.kind == ExternalKind::Func {
                    self.signature(export.index)?
                } else {
                    None
                };
                native.record(export.name, signature);
            }
        }
        Ok(native.requested())
    }

    /// Finds one exported function's type, accounting for function imports before definitions.
    fn signature(&self, mut function: u32) -> Result<Option<Signature>> {
        let mut type_index = None;
        if let Some(imports) = &self.imports {
            for import in imports.clone().into_imports() {
                if let TypeRef::Func(index) | TypeRef::FuncExact(index) = import?.ty {
                    if function == 0 {
                        type_index = Some(index);
                        break;
                    }
                    function -= 1;
                }
            }
        }
        if type_index.is_none()
            && let Some(functions) = &self.functions
        {
            type_index = functions
                .clone()
                .into_iter()
                .nth(function as usize)
                .transpose()?;
        }
        let (Some(mut index), Some(types)) = (type_index, &self.types) else {
            return Ok(None);
        };
        for group in types.clone() {
            for ty in group?.into_types() {
                if index == 0 {
                    return Ok(match ty.composite_type.inner {
                        CompositeInnerType::Func(ty) => Some(Signature {
                            params: ty.params().len(),
                            results: ty.results().len(),
                            i32_only: ty
                                .params()
                                .iter()
                                .chain(ty.results())
                                .all(|ty| *ty == ValType::I32),
                        }),
                        _ => None,
                    });
                }
                index -= 1;
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_requests_and_runtime_negotiation_share_required_grants() {
        let module = Module::new(
            &wasmtime::Engine::default(),
            r#"(module
                (func (export "native-init") (param i32))
                (func (export "frame") (param i32 i32) (result i32) i32.const 0)
                (func (export "menu") (param i32 i32) (result i32) i32.const 0))"#,
        )
        .unwrap();
        let exports = NativeExports::from_module(&module);
        let requested = exports.requested();
        assert_eq!(
            requested,
            ModGrants {
                players: true,
                camera: true,
                controls: true,
                ..Default::default()
            }
        );
        for (players, camera, controls) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, false),
            (true, true, true),
        ] {
            let granted = ModGrants {
                players,
                camera,
                controls,
                ..Default::default()
            };
            let saved = granted.clone();
            assert_eq!(
                exports.negotiate(&granted).unwrap(),
                if players && camera {
                    mod_api::native_frame::CAMERA_PLAYERS
                } else {
                    0
                } | if controls {
                    mod_api::native_menu::CONTROLS
                } else {
                    0
                }
            );
            assert_eq!(
                granted, saved,
                "discovery and negotiation never grant authority"
            );
        }
    }

    #[test]
    fn compiled_exports_reject_incomplete_or_invalid_native_abis() {
        for source in [
            r#"(module (func (export "native-init") (param i32)))"#,
            r#"(module (func (export "menu") (param i32 i32) (result i32) i32.const 0))"#,
            r#"(module
                (global (export "native-init") i32 (i32.const 0))
                (func (export "menu") (param i32 i32) (result i32) i32.const 0))"#,
            r#"(module
                (func (export "native-init") (param i32))
                (func (export "menu") (param i32 i64) (result i32) i32.const 0))"#,
        ] {
            let module = Module::new(&wasmtime::Engine::default(), source).unwrap();
            let exports = NativeExports::from_module(&module);
            assert_eq!(exports.requested(), ModGrants::default());
            assert!(exports.negotiate(&ModGrants::default()).is_err());
        }
    }
}
