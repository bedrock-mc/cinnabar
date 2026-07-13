#[derive(Debug)]
pub struct LiquidShaderContract {
    face_xz: [[f32; 2]; 24],
    base_uv: [[f32; 2]; 24],
    flow_angle_direction: f32,
    flow_angle_offset: f32,
    side_height_bias: f32,
    side_height_scale: f32,
    falling_scroll_direction: f32,
    falling_scroll_ticks: f32,
    flow_face: usize,
    side_face_mask: u32,
    falling_face_mask: u32,
    flow_direction_operator: f32,
    flow_offset_operator: f32,
    side_scale_operator: f32,
    falling_assignment_operator: f32,
}

impl LiquidShaderContract {
    pub fn parse(shader: &str) -> Self {
        validate_structural_wiring(shader);
        Self {
            face_xz: parse_vec2_table(shader, "LIQUID_FACE_XZ"),
            base_uv: parse_vec2_table(shader, "LIQUID_BASE_UV"),
            flow_angle_direction: parse_f32(shader, "FLOW_ANGLE_DIRECTION"),
            flow_angle_offset: parse_f32(shader, "FLOW_ANGLE_OFFSET"),
            side_height_bias: parse_f32(shader, "SIDE_HEIGHT_BIAS"),
            side_height_scale: parse_f32(shader, "SIDE_HEIGHT_SCALE"),
            falling_scroll_direction: parse_f32(shader, "FALLING_SCROLL_DIRECTION"),
            falling_scroll_ticks: parse_f32(shader, "FALLING_SCROLL_TICKS"),
            flow_face: parse_u32(shader, "FLOW_FACE") as usize,
            side_face_mask: parse_u32(shader, "SIDE_FACE_MASK"),
            falling_face_mask: parse_u32(shader, "FALLING_FACE_MASK"),
            flow_direction_operator: parse_flow_direction_operator(shader),
            flow_offset_operator: parse_flow_offset_operator(shader),
            side_scale_operator: parse_side_scale_operator(shader),
            falling_assignment_operator: parse_falling_assignment_operator(shader),
        }
    }

    pub fn corner(&self, face: usize, corner: usize, origin: [f32; 3], height: u8) -> [f32; 3] {
        let xz = self.face_xz[face * 4 + corner];
        [
            origin[0] + xz[0],
            origin[1] + f32::from(height) / 255.0,
            origin[2] + xz[1],
        ]
    }

    pub fn uv(
        &self,
        face: usize,
        corner: usize,
        height: u8,
        flow: [i8; 2],
        falling: bool,
        clock: [f32; 2],
    ) -> [f32; 2] {
        let mut uv = self.base_uv[face * 4 + corner];
        if self.side_face_mask & (1 << face) != 0 {
            uv[1] = self.side_height_bias
                + self.side_scale_operator * self.side_height_scale * f32::from(height) / 255.0;
        }
        if face == self.flow_face && flow != [0, 0] {
            let radians = self.flow_direction_operator
                * self.flow_angle_direction
                * f32::from(flow[1]).atan2(f32::from(flow[0]))
                + self.flow_offset_operator * self.flow_angle_offset;
            let centered = [uv[0] - 0.5, uv[1] - 0.5];
            uv = [
                0.5 + centered[0] * radians.cos() - centered[1] * radians.sin(),
                0.5 + centered[0] * radians.sin() + centered[1] * radians.cos(),
            ];
        }
        if falling && self.falling_face_mask & (1 << face) != 0 {
            let phase = ((clock[0] + clock[1].clamp(0.0, 0.999_999_94))
                / self.falling_scroll_ticks)
                .fract();
            uv[1] += self.falling_assignment_operator * self.falling_scroll_direction * phase;
        }
        uv
    }
}

fn parse_f32(shader: &str, name: &str) -> f32 {
    let prefix = format!("const {name}: f32 = ");
    let start = shader
        .find(&prefix)
        .unwrap_or_else(|| panic!("liquid shader is missing {name}"))
        + prefix.len();
    let end = shader[start..]
        .find(';')
        .map(|offset| start + offset)
        .expect("shader constant must end with a semicolon");
    shader[start..end]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be a literal f32"))
}

fn parse_u32(shader: &str, name: &str) -> u32 {
    let prefix = format!("const {name}: u32 = ");
    let start = shader
        .find(&prefix)
        .unwrap_or_else(|| panic!("liquid shader is missing {name}"))
        + prefix.len();
    let end = shader[start..]
        .find(';')
        .map(|offset| start + offset)
        .expect("shader constant must end with a semicolon");
    shader[start..end]
        .trim()
        .strip_suffix('u')
        .expect("u32 shader constant must have a u suffix")
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be a literal u32"))
}

fn compact(shader: &str) -> String {
    shader.split_whitespace().collect()
}

fn validate_structural_wiring(shader: &str) {
    let shader = compact(shader);
    let requirements = [
        (
            "LIQUID_FACE_XZ[face*4u+corner]",
            "face/corner geometry-table addressing",
            1,
        ),
        (
            "LIQUID_BASE_UV[face*4u+corner]",
            "face/corner UV-table addressing",
            1,
        ),
        ("((SIDE_FACE_MASK>>face)&1u)!=0u", "side-face mask test", 1),
        ("face==FLOW_FACE", "flow-face test", 1),
        (
            "falling&&((FALLING_FACE_MASK>>face)&1u)!=0u",
            "falling-face mask test",
            1,
        ),
        (
            "f32((height_word>>(corner*8u))&255u)/255.0",
            "packed u8 height decode",
            2,
        ),
        (
            "signed_i8(geometry,16u),signed_i8(geometry,24u)",
            "signed X/Z flow call wiring",
            1,
        ),
        (
            "bitcast<i32>((word>>shift)<<24u)>>24u",
            "signed i8 sign extension",
            1,
        ),
    ];
    for (expression, label, expected_count) in requirements {
        assert_eq!(
            shader.matches(expression).count(),
            expected_count,
            "liquid shader changed required {label}: expected {expected_count} occurrence(s) of {expression}",
        );
    }
}

fn parse_flow_direction_operator(shader: &str) -> f32 {
    let shader = compact(shader);
    let positive = "letradians=FLOW_ANGLE_DIRECTION*atan2(f32(flow_z),f32(flow_x))";
    let negative = "letradians=-FLOW_ANGLE_DIRECTION*atan2(f32(flow_z),f32(flow_x))";
    if shader.contains(positive) {
        1.0
    } else if shader.contains(negative) {
        -1.0
    } else {
        panic!("unsupported liquid flow-angle direction expression")
    }
}

fn parse_flow_offset_operator(shader: &str) -> f32 {
    let shader = compact(shader);
    let positive = "atan2(f32(flow_z),f32(flow_x))+FLOW_ANGLE_OFFSET";
    let negative = "atan2(f32(flow_z),f32(flow_x))-FLOW_ANGLE_OFFSET";
    if shader.contains(positive) {
        1.0
    } else if shader.contains(negative) {
        -1.0
    } else {
        panic!("unsupported liquid flow-angle offset expression")
    }
}

fn parse_side_scale_operator(shader: &str) -> f32 {
    let shader = compact(shader);
    if shader.contains("uv.y=SIDE_HEIGHT_BIAS+SIDE_HEIGHT_SCALE*height") {
        1.0
    } else if shader.contains("uv.y=SIDE_HEIGHT_BIAS-SIDE_HEIGHT_SCALE*height") {
        -1.0
    } else {
        panic!("unsupported liquid side-height expression")
    }
}

fn parse_falling_assignment_operator(shader: &str) -> f32 {
    let shader = compact(shader);
    if shader.contains("uv.y+=FALLING_SCROLL_DIRECTION*falling_phase") {
        1.0
    } else if shader.contains("uv.y-=FALLING_SCROLL_DIRECTION*falling_phase") {
        -1.0
    } else {
        panic!("unsupported liquid falling-scroll expression")
    }
}

fn parse_vec2_table(shader: &str, name: &str) -> [[f32; 2]; 24] {
    let prefix = format!("const {name}: array<vec2<f32>, 24> = array(");
    let start = shader
        .find(&prefix)
        .unwrap_or_else(|| panic!("liquid shader is missing {name}"))
        + prefix.len();
    let end = shader[start..]
        .find(");")
        .map(|offset| start + offset)
        .expect("shader table must end with );");
    let mut values = Vec::new();
    let mut remaining = &shader[start..end];
    while let Some(vector) = remaining.find("vec2(") {
        remaining = &remaining[vector + "vec2(".len()..];
        let close = remaining.find(')').expect("vec2 literal must close");
        let components = remaining[..close]
            .split(',')
            .map(str::trim)
            .map(|component| {
                component
                    .parse::<f32>()
                    .expect("vec2 component must be literal")
            })
            .collect::<Vec<_>>();
        assert_eq!(components.len(), 2, "vec2 literal must have two components");
        values.push([components[0], components[1]]);
        remaining = &remaining[close + 1..];
    }
    values.try_into().unwrap_or_else(|values: Vec<_>| {
        panic!("{name} must contain 24 vec2 values, got {}", values.len())
    })
}

#[cfg(test)]
mod tests {
    use super::LiquidShaderContract;

    const SHADER: &str = include_str!("../../src/liquid.wgsl");

    fn assert_rejects_mutation(from: &str, to: &str) {
        // `include_str!` preserves checkout line endings. Normalize the source
        // before applying multi-line semantic mutations so this contract tests
        // identical shader text on Windows and Unix worktrees.
        let shader = SHADER.replace("\r\n", "\n");
        let mutated = shader.replacen(from, to, 1);
        assert_ne!(mutated, shader, "mutation source must exist: {from}");
        assert!(
            std::panic::catch_unwind(|| LiquidShaderContract::parse(&mutated)).is_err(),
            "contract accepted structural shader mutation: {from} -> {to}",
        );
    }

    #[test]
    fn decoder_observes_shader_operator_mutations_behaviorally() {
        let baseline = LiquidShaderContract::parse(SHADER);

        let angle_mutation = SHADER.replacen(
            "FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x))",
            "-FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x))",
            1,
        );
        let changed_angle = LiquidShaderContract::parse(&angle_mutation);
        assert_ne!(
            baseline.uv(3, 0, 255, [0, 1], false, [0.0, 0.0]),
            changed_angle.uv(3, 0, 255, [0, 1], false, [0.0, 0.0]),
        );

        let side_mutation = SHADER.replacen(
            "SIDE_HEIGHT_BIAS + SIDE_HEIGHT_SCALE * height",
            "SIDE_HEIGHT_BIAS - SIDE_HEIGHT_SCALE * height",
            1,
        );
        let changed_side = LiquidShaderContract::parse(&side_mutation);
        assert_ne!(
            baseline.uv(0, 1, 128, [0, 0], false, [0.0, 0.0]),
            changed_side.uv(0, 1, 128, [0, 0], false, [0.0, 0.0]),
        );

        let falling_mutation = SHADER.replacen(
            "uv.y += FALLING_SCROLL_DIRECTION * falling_phase",
            "uv.y -= FALLING_SCROLL_DIRECTION * falling_phase",
            1,
        );
        let changed_falling = LiquidShaderContract::parse(&falling_mutation);
        assert_ne!(
            baseline.uv(0, 1, 128, [0, 0], true, [4.0, 0.0]),
            changed_falling.uv(0, 1, 128, [0, 0], true, [4.0, 0.0]),
        );
    }

    #[test]
    fn decoder_rejects_shader_address_mask_height_and_flow_wiring_mutations() {
        assert_rejects_mutation(
            "LIQUID_FACE_XZ[face * 4u + corner]",
            "LIQUID_FACE_XZ[corner * 4u + face]",
        );
        assert_rejects_mutation(
            "LIQUID_BASE_UV[face * 4u + corner]",
            "LIQUID_BASE_UV[corner * 4u + face]",
        );
        assert_rejects_mutation("SIDE_FACE_MASK >> face", "SIDE_FACE_MASK >> corner");
        assert_rejects_mutation("face == FLOW_FACE", "face != FLOW_FACE");
        assert_rejects_mutation("FALLING_FACE_MASK >> face", "FALLING_FACE_MASK >> corner");
        assert_rejects_mutation(
            "f32((height_word >> (corner * 8u)) & 255u) / 255.0",
            "f32((height_word >> (corner * 8u)) & 255u) / 256.0",
        );
        assert_rejects_mutation(
            "signed_i8(geometry, 16u),\n        signed_i8(geometry, 24u),",
            "signed_i8(geometry, 24u),\n        signed_i8(geometry, 16u),",
        );
        assert_rejects_mutation(
            "bitcast<i32>((word >> shift) << 24u) >> 24u",
            "bitcast<i32>((word >> shift) << 24u) >> 23u",
        );
    }
}
