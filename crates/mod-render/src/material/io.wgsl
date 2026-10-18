struct MaterialInput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local_position: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) distance: f32,
    @location(4) @interpolate(flat) params0: vec4<f32>,
    @location(5) @interpolate(flat) params1: vec4<f32>,
    @location(6) @interpolate(flat) params2: vec4<f32>,
    @location(7) @interpolate(flat) params3: vec4<f32>,
}
struct Surface { color: vec3<f32>, coverage: f32, emission: vec3<f32> }
fn param(input: MaterialInput, index: u32) -> f32 {
    switch min(index, 15u) / 4u {
        case 0u: { return input.params0[index % 4u]; }
        case 1u: { return input.params1[index % 4u]; }
        case 2u: { return input.params2[index % 4u]; }
        default: { return input.params3[index % 4u]; }
    }
}
