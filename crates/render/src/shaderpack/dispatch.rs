//! Computes pack dispatch sizes and checks them before frame recording.

use shaderpack::ComputeDispatch;

/// Resolves fixed or screen-relative dispatch counts without changing fixed zero counts.
pub(super) fn groups(dispatch: &ComputeDispatch, screen: [u32; 2]) -> [u32; 3] {
    dispatch.work_groups.unwrap_or_else(|| {
        let scale = dispatch.work_groups_render.unwrap_or([1.0; 2]);
        std::array::from_fn(|axis| {
            if axis == 2 {
                1
            } else {
                ((screen[axis] as f32 * scale[axis]).ceil() as u32)
                    .div_ceil(dispatch.workgroup_size[axis].max(1))
                    .max(1)
            }
        })
    })
}

/// Rejects counts the device cannot record, including those produced after a resize.
pub(super) fn validate(
    dispatch: &ComputeDispatch,
    screen: [u32; 2],
    limits: &wgpu::Limits,
) -> Result<(), String> {
    let counts = groups(dispatch, screen);
    let limit = limits.max_compute_workgroups_per_dimension;
    if counts.iter().any(|&count| count > limit) {
        return Err(format!(
            "compute dispatch {counts:?} exceeds device limit {limit}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_dispatches_allow_the_limit_and_zero_but_reject_any_over_limit_axis() {
        let limits = wgpu::Limits::default();
        let limit = limits.max_compute_workgroups_per_dimension;
        let mut dispatch = ComputeDispatch {
            workgroup_size: [1; 3],
            work_groups: Some([limit, 0, 1]),
            work_groups_render: None,
        };
        assert_eq!(groups(&dispatch, [1920, 1080]), [limit, 0, 1]);
        assert!(validate(&dispatch, [1920, 1080], &limits).is_ok());
        for axis in 0..3 {
            let mut counts = [1; 3];
            counts[axis] = limit + 1;
            dispatch.work_groups = Some(counts);
            assert!(validate(&dispatch, [1, 1], &limits).is_err());
        }
    }

    #[test]
    fn relative_dispatches_round_up_local_groups_and_revalidate_resizes() {
        let limits = wgpu::Limits {
            max_compute_workgroups_per_dimension: 8,
            ..Default::default()
        };
        let dispatch = ComputeDispatch {
            workgroup_size: [4, 4, 1],
            work_groups: None,
            work_groups_render: Some([0.5, 1.0]),
        };
        assert_eq!(groups(&dispatch, [63, 31]), [8, 8, 1]);
        assert!(validate(&dispatch, [63, 31], &limits).is_ok());
        assert!(validate(&dispatch, [65, 31], &limits).is_err());
        assert!(validate(&dispatch, [63, 33], &limits).is_err());
    }
}
