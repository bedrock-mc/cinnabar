#define_import_path cinnabar::enhanced_pbr
// ENHANCED_PBR_CONSTANTS

struct MaterialBasis { tangent: vec3<f32>, bitangent: vec3<f32>, normal: vec3<f32>, scale: vec2<f32> }
struct MaterialResponse { roughness: f32, metallic: f32, fzero: vec3<f32>, emission: f32, subsurface: f32, porosity: f32 }

// Derivatives follow each authored UV rotation, mirror and model transform.
fn material_basis(normal: vec3<f32>, position_dx: vec3<f32>, position_dy: vec3<f32>, uv_dx: vec2<f32>, uv_dy: vec2<f32>) -> MaterialBasis {
    let determinant = uv_dx.x * uv_dy.y - uv_dx.y * uv_dy.x;
    var tangent = cross(vec3(0.0,1.0,0.0), normal);
    if (dot(tangent,tangent) < 0.01) { tangent = cross(vec3(0.0,0.0,1.0), normal); }
    tangent = normalize(tangent);
    var bitangent = cross(normal, tangent);
    var scale = vec2(1.0);
    if (abs(determinant) > 1.0e-10) {
        let u = (position_dx * uv_dy.y - position_dy * uv_dx.y) / determinant;
        let v = (position_dy * uv_dx.x - position_dx * uv_dy.x) / determinant;
        let projected = u - normal * dot(normal,u);
        if (dot(projected,projected) > 1.0e-10) {
            tangent = normalize(projected);
            bitangent = cross(normal,tangent) * select(-1.0,1.0,dot(cross(normal,tangent),v)>=0.0);
            scale = max(vec2(length(u),length(v)),vec2(0.01));
        }
    }
    return MaterialBasis(tangent,bitangent,normal,scale);
}

fn material_normal(sampled:vec4<f32>, basis:MaterialBasis)->vec3<f32> {
    let xy = sampled.xy * 2.0 - vec2(1.0);
    let z = sqrt(max(1.0-dot(xy,xy),0.0001));
    return normalize(basis.tangent*xy.x + basis.bitangent*xy.y + basis.normal*z);
}

fn conductor_fzero(n:vec3<f32>, k:vec3<f32>)->vec3<f32> {
    let lower=n-vec3(1.0); let upper=n+vec3(1.0);
    return (lower*lower+k*k)/(upper*upper+k*k);
}
fn material_metal_fzero(code:u32, albedo:vec3<f32>)->vec3<f32> {
    var optical=vec3(1.0);
    switch code {
        case 230u: { optical=conductor_fzero(vec3(2.9114,2.9497,2.5845),vec3(3.0893,2.9318,2.7670)); }
        case 231u: { optical=conductor_fzero(vec3(0.18299,0.42108,1.3734),vec3(3.4242,2.3459,1.7704)); }
        case 232u: { optical=conductor_fzero(vec3(1.3456,0.96521,0.61722),vec3(7.4746,6.3995,5.3031)); }
        case 233u: { optical=conductor_fzero(vec3(3.1071,3.1812,2.3230),vec3(3.3314,3.3291,3.1350)); }
        case 234u: { optical=conductor_fzero(vec3(0.27105,0.67693,1.3164),vec3(3.6092,2.6248,2.2921)); }
        case 235u: { optical=conductor_fzero(vec3(1.91,1.83,1.44),vec3(3.51,3.4,3.18)); }
        case 236u: { optical=conductor_fzero(vec3(2.3757,2.0847,1.8453),vec3(4.2655,3.7153,3.1365)); }
        case 237u: { optical=conductor_fzero(vec3(0.15943,0.14512,0.13547),vec3(3.9291,3.19,2.3808)); }
        default: {}
    }
    return clamp(optical * albedo,vec3(0.0),vec3(1.0));
}
fn material_response(sampled:vec4<f32>, flags:u32, albedo:vec3<f32>)->MaterialResponse {
    var metallic=clamp(sampled.x,0.0,1.0);
    var fzero=mix(vec3(0.04),albedo,metallic);
    var subsurface=clamp(sampled.a,0.0,1.0); var porosity=0.0;
    if ((flags&PBR_REF_LABPBR)!=0u) {
        let code=u32(round(sampled.x*255.0));
        metallic=select(0.0,1.0,code>=230u);
        fzero=select(vec3(clamp(sampled.x,0.0,1.0)),material_metal_fzero(code,albedo),metallic>0.5);
        let packed_auxiliary=sampled.a*255.0;
        subsurface=clamp((packed_auxiliary-65.0)/190.0,0.0,1.0);
        porosity=select(0.0,clamp(packed_auxiliary/64.0,0.0,1.0),packed_auxiliary<=64.0);
    }
    if ((flags&PBR_REF_SUBSURFACE)==0u) { subsurface=0.0; }
    return MaterialResponse(clamp(sampled.z,0.045,1.0),metallic,fzero,clamp(sampled.y,0.0,1.0),subsurface,porosity);
}

// Relief fades before it becomes subpixel; silhouettes and translucent cards keep geometry.
fn material_relief_weight(flags:u32, allowed:bool, distance:f32, footprint:f32, incidence:f32)->f32 {
    if (flags==0xffffffffu || (flags&PBR_REF_HEIGHT)==0u || !allowed) {return 0.0;}
    return (1.0-smoothstep(10.0,18.0,distance))*(1.0-smoothstep(0.006,0.018,footprint))*smoothstep(0.12,0.3,incidence);
}
fn material_relief_ray(view:vec3<f32>,basis:MaterialBasis,weight:f32)->vec2<f32> {
    let tangent_view=vec3(dot(view,basis.tangent),dot(view,basis.bitangent),abs(dot(view,basis.normal)));
    return -tangent_view.xy/max(tangent_view.z,0.12)*PBR_HEIGHT_SCALE*weight/basis.scale;
}
