//! Optional quarter-level terrain light samples; native records retain their format.

use super::*;

pub const JAVA_SAMPLE_FLAG: u16 = 1 << 15;

/// Bakes four corner averages with independent sky and block channels.
pub(super) fn bake<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
    check_bounds: bool,
) -> PackedQuadLighting {
    let (normal, tangent_a, tangent_b) = face_basis(face);
    let outward = add_normal(block, normal);
    let axis = normal
        .iter()
        .position(|&value| value != 0)
        .expect("face axis");
    let boundary = !check_bounds
        || positions.iter().all(|p| {
            if normal[axis] < 0 {
                p[axis] <= 0
            } else {
                p[axis] >= 256
            }
        });
    let plane = if boundary { normal } else { [0; 3] };
    let center = inputs.sample(if boundary || !inputs.occludes(outward) {
        outward
    } else {
        block
    });
    let center_shade = inputs.shade_darkened(if boundary { outward } else { block });
    PackedQuadLighting::new(positions.map(|position| {
        let side_a = offset(
            block,
            plane,
            tangent_a,
            corner_sign(position[tangent_a]),
            None,
        );
        let side_b = offset(
            block,
            plane,
            tangent_b,
            corner_sign(position[tangent_b]),
            None,
        );
        let a = inputs.cell(side_a);
        let b = inputs.cell(side_b);
        let diagonal = if inputs.occludes(add_normal(side_a, normal))
            && inputs.occludes(add_normal(side_b, normal))
        {
            a
        } else {
            inputs.cell(offset(
                block,
                plane,
                tangent_a,
                corner_sign(position[tangent_a]),
                Some((tangent_b, corner_sign(position[tangent_b]))),
            ))
        };
        let ao = u8::from(center_shade)
            + u8::from(a.shade_darkened)
            + u8::from(b.shade_darkened)
            + u8::from(diagonal.shade_darkened);
        average([a.light, b.light, diagonal.light], center, ao)
    }))
}

/// Repairs missing corner channels in a lit face before retaining quarter-level precision.
fn average(mut corners: [MeshLightSample; 3], center: MeshLightSample, ao: u8) -> u16 {
    if center.block() > 2 || center.sky() > 2 {
        for sample in &mut corners {
            if sample.0 == 0 {
                *sample = center;
            } else if sample.sky() == 0 {
                *sample =
                    MeshLightSample::try_new(sample.block(), center.sky()).expect("bounded sky");
            }
        }
    }
    let block = u16::from(center.block())
        + corners
            .iter()
            .map(|sample| u16::from(sample.block()))
            .sum::<u16>();
    let sky = u16::from(center.sky())
        + corners
            .iter()
            .map(|sample| u16::from(sample.sky()))
            .sum::<u16>();
    JAVA_SAMPLE_FLAG | block | (sky << 6) | (u16::from(ao) << 12)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn averaging_keeps_fractional_light_and_repairs_only_missing_channels() {
        let sample = |block, sky| MeshLightSample::try_new(block, sky).unwrap();
        let packed = average([sample(1, 4), sample(2, 5), sample(3, 6)], sample(4, 7), 2);
        assert_eq!(packed & 63, 10);
        assert_eq!((packed >> 6) & 63, 22);
        assert_eq!((packed >> 12) & 7, 2);
        assert_ne!(packed & JAVA_SAMPLE_FLAG, 0);
        let repaired = average([sample(0, 0), sample(2, 0), sample(0, 3)], sample(4, 7), 0);
        assert_eq!(repaired & 63, 10);
        assert_eq!((repaired >> 6) & 63, 24);
    }
}
