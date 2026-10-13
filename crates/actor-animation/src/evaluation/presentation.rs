use super::*;
use crate::render_frame::camera;

impl PresentationReads {
    /// Combines inputs conservatively across authored branches and assignments.
    pub(super) fn union(self, other: Self) -> Self {
        Self {
            camera: self.camera || other.camera,
            swing: self.swing || other.swing,
        }
    }
}

impl VariableLayout {
    /// Resolves camera and swing dependencies together, including render-controller writes.
    pub(super) fn bind_presentation_inputs(&mut self, assets: &RuntimeEntityAssets) {
        self.presentation_symbols = (0..assets.molang_symbols().len())
            .map(|symbol| PresentationReads {
                camera: camera::camera_symbol(assets, symbol as u32, false),
                swing: camera::camera_symbol(assets, symbol as u32, true),
            })
            .collect();
        self.expression_inputs =
            vec![PresentationReads::default(); assets.molang_expressions().len()];
        loop {
            let mut changed = false;
            for (index, expression) in assets.molang_expressions().iter().enumerate() {
                let first = expression.first_op as usize;
                let ops = &assets.molang_ops()[first..first + usize::from(expression.op_count)];
                let mut inputs = PresentationReads::default();
                for op in ops {
                    let symbol = match op {
                        MolangOp::LoadVariable(symbol) | MolangOp::LoadQuery(symbol) => *symbol,
                        MolangOp::CallQuery(call) => call.symbol,
                        MolangOp::Coalesce(branch) => branch.symbol,
                        _ => continue,
                    };
                    inputs = inputs.union(self.symbol_inputs(symbol));
                }
                self.expression_inputs[index] = inputs;
                // Any branch may run; an uncertain write must keep live presentation inputs.
                for op in ops {
                    if let MolangOp::StoreVariable(symbol) = op {
                        let previous = self.presentation_symbols[*symbol as usize];
                        let next = previous.union(inputs);
                        changed |= next.camera != previous.camera || next.swing != previous.swing;
                        self.presentation_symbols[*symbol as usize] = next;
                    }
                }
            }
            // The native fallback copies the authored factor after pre_animation.
            if let (Some(source), Some(target)) = (
                self.engine.first_person_rotation_factor,
                self.engine.first_person_item_rotation_factor,
            ) {
                let source = self.presentation_symbols[self.variable_base + source];
                let target = &mut self.presentation_symbols[self.variable_base + target];
                let next = target.union(source);
                changed |= next.camera != target.camera || next.swing != target.swing;
                *target = next;
            }
            if !changed {
                break;
            }
        }
    }

    /// Unknown reads conservatively require live camera and swing inputs.
    pub(crate) fn symbol_inputs(&self, symbol: u32) -> PresentationReads {
        self.presentation_symbols
            .get(symbol as usize)
            .copied()
            .unwrap_or(PresentationReads {
                camera: true,
                swing: true,
            })
    }

    /// Shares the assignment closure between pose admission and selected-layer sampling.
    pub(crate) fn expression_inputs(&self, expression: usize) -> PresentationReads {
        self.expression_inputs
            .get(expression)
            .copied()
            .unwrap_or(PresentationReads {
                camera: true,
                swing: true,
            })
    }
}
