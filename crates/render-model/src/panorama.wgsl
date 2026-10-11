// The menu panorama: each pixel's view ray picks a cube face and samples it,
// which is the same image as rasterizing the cube from its centre.

struct Panorama {
    // yaw, pitch (radians), tan(half vertical fov), aspect (width / height)
    view: vec4<f32>,
    // overlay tint, straight alpha
    tint: vec4<f32>,
}

@group(0) @binding(0) var<uniform> panorama: Panorama;
@group(0) @binding(1) var faces: texture_2d_array<f32>;
@group(0) @binding(2) var face_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn panorama_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

// Face order follows the pack: 0 front (-Z), 1 right (+X), 2 back (+Z),
// 3 left (-X), 4 top (+Y), 5 bottom (-Y); each face is seen from inside.
fn face_uv(dir: vec3<f32>) -> vec3<f32> {
    let a = abs(dir);
    if a.y >= a.x && a.y >= a.z {
        // Top's lower edge and bottom's upper edge meet the front face.
        if dir.y > 0.0 {
            return vec3<f32>(dir.x / a.y, -dir.z / a.y, 4.0);
        }
        return vec3<f32>(dir.x / a.y, dir.z / a.y, 5.0);
    }
    if a.x >= a.z {
        if dir.x > 0.0 {
            return vec3<f32>(dir.z / a.x, -dir.y / a.x, 1.0);
        }
        return vec3<f32>(-dir.z / a.x, -dir.y / a.x, 3.0);
    }
    if dir.z < 0.0 {
        return vec3<f32>(dir.x / a.z, -dir.y / a.z, 0.0);
    }
    return vec3<f32>(-dir.x / a.z, -dir.y / a.z, 2.0);
}

@fragment
fn panorama_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let yaw = panorama.view.x;
    let pitch = panorama.view.y;
    let tan_half = panorama.view.z;
    let aspect = panorama.view.w;
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    var dir = vec3<f32>(ndc.x * tan_half * aspect, ndc.y * tan_half, -1.0);
    // Pitch about X (positive looks up), then yaw about Y (positive turns right).
    let cp = cos(pitch);
    let sp = sin(pitch);
    dir = vec3<f32>(dir.x, dir.y * cp - dir.z * sp, dir.y * sp + dir.z * cp);
    let cy = cos(yaw);
    let sy = sin(yaw);
    dir = vec3<f32>(dir.x * cy - dir.z * sy, dir.y, dir.x * sy + dir.z * cy);
    let face = face_uv(dir);
    let uv = face.xy * 0.5 + vec2<f32>(0.5, 0.5);
    let color = textureSample(faces, face_sampler, uv, i32(face.z)).rgb;
    let tinted = mix(color, panorama.tint.rgb, panorama.tint.a);
    return vec4<f32>(tinted, 1.0);
}
