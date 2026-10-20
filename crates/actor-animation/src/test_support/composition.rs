//! Original changing-pose fixtures for comparing composition kernels.

use super::super::{
    pose::{Composition, LocalDelta},
    *,
};

/// An original rig with nonzero animation and retained output for kernel measurements.
pub struct CompositionFixture {
    bones: Vec<RuntimeBone>,
    local: Vec<LocalDelta>,
    program: Composition,
    output: Vec<BoneTransform>,
}

impl CompositionFixture {
    /// Builds a branching hierarchy in child-first storage with rotated, scaled parents.
    pub fn new(count: usize) -> Self {
        let bones: Vec<_> = (0..count)
            .map(|index| RuntimeBone {
                parent: (index + 1 < count).then(|| (index + count).div_ceil(2)),
                pivot: [index as f32 * 0.5, index as f32 * 1.25, 0.25],
                rotation: [index as f32 * 0.7, 3.0, 2.0],
                ..Default::default()
            })
            .collect();
        let local = (0..count)
            .map(|index| LocalDelta {
                rotation: [index as f32 * 0.7, 3.0, 2.0],
                translation: [0.1, 0.2, 0.3],
                scale: [1.001, 0.999, 1.0],
                ..Default::default()
            })
            .collect();
        let program = Composition::new(&bones).unwrap();
        Self {
            bones,
            local,
            program,
            output: vec![BoneTransform::default(); count],
        }
    }

    /// Runs the retained recursive implementation, including its allocation and validation.
    pub fn recursive(&self) -> Vec<BoneTransform> {
        pose::compose_pose(&self.bones, &self.local).unwrap()
    }

    /// Runs the production program into a retained caller-owned output span.
    pub fn flat(&mut self) -> &[BoneTransform] {
        self.program
            .compose_into(&self.bones, |index| self.local[index], &mut self.output)
            .unwrap();
        &self.output
    }
}
